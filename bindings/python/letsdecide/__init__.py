"""One request shape for every decision API. See README.md."""

from __future__ import annotations

import inspect
import json
import re
from typing import Any

from . import _core

__all__ = [
    "BUILTIN",
    "MAX_DEPTH",
    "AsyncClient",
    "Client",
    "LetsDecideError",
    "capabilities",
    "check",
    "classify_http",
    "dialect",
    "mock_engine",
    "resolve_dialect",
    "validate",
]


class LetsDecideError(Exception):
    def __init__(
        self, code, message, status=None, retry_after_ms=None, retryable=False, details=None
    ):
        super().__init__(message)
        self.code = code
        self.status = status
        self.retry_after_ms = retry_after_ms
        self.retryable = retryable
        # Safe to log: names, ids, counts, MIME types. Never keys or content.
        self.details = details


def _unwrap(text: str) -> Any:
    """The core returns {ok} or {error}; unwrap or raise."""
    out = json.loads(text)
    if "error" in out:
        raise LetsDecideError(**out["error"])
    return out["ok"]


_TIMEOUT = 60  # httpx defaults to 5s; decisions on media take longer


def _httpx():
    try:
        import httpx
    except ImportError as e:
        raise ImportError("install `letsdecide[httpx]` or pass your own client") from e
    return httpx


#: Built-in provider names; custom providers can't take them without `replace`.
BUILTIN = ("jev", "clef", "gemini", "openai", "systemone")

#: How deep engines may call back into the client (a panel of panels...).
MAX_DEPTH = 4

_FIELDS = ("api_key", "access_token", "base_url", "account_id", "headers", "options")

_NAME = re.compile(r"[a-z0-9_-]{1,64}")
_HEADER_NAME = re.compile(r"[!#$%&'*+\-.^_`|~0-9A-Za-z]+")


def _pairs(h):
    if not h:
        return []
    return [list(p) for p in (h.items() if isinstance(h, dict) else h)]


def validate(request: dict, capabilities: dict | None = None) -> None:
    """Validates `request` and, if given, checks it against `capabilities`."""
    _unwrap(_core.validate(json.dumps(request), json.dumps(capabilities)))


def check(request: dict, response: dict) -> dict:
    """Checks a canonical `response` against `request`; returns it with defaults."""
    return _unwrap(_core.check(json.dumps(request), json.dumps(response)))


def classify_http(response: dict) -> LetsDecideError:
    """The error a built-in adapter raises for this non-2xx reply
    (`{status, headers, body}`). Return it to raise it."""
    reply = {
        "status": response["status"],
        "headers": _pairs(response.get("headers")),
        "body": response.get("body") or "",
    }
    return LetsDecideError(**_unwrap(_core.classify_http(json.dumps(reply))))


def capabilities(model: str) -> dict:
    """Capabilities of a built-in `"provider/model"`."""
    return _unwrap(_core.capabilities(model))


def dialect(name: str) -> dict:
    """A built-in dialect (`jev`, `clef`, `gemini`, `openai`, `systemone`) as a
    dict, to read or extend."""
    return _unwrap(_core.dialect(name))


def resolve_dialect(spec: dict) -> dict:
    """Validates a dialect and expands `extends`. Raises `LetsDecideError`."""
    return _unwrap(_core.resolve_dialect(json.dumps(spec)))


class _DialectCodec:
    """A dialect run by the core, as a codec. Validated when the client is made."""

    def __init__(self, spec):
        self._dialect = json.dumps(resolve_dialect(spec))
        self.capabilities = json.loads(self._dialect)["capabilities"]

    def build(self, model, request, config):
        return _unwrap(
            _core.build_dialect(self._dialect, model, json.dumps(request), json.dumps(config))
        )

    def parse(self, request, response):
        return _unwrap(
            _core.parse_dialect(self._dialect, json.dumps(request), json.dumps(response))
        )


def _from_user_code(name, e):
    """Tags our errors with the provider; wraps anything else as `ProviderError`.
    The original is kept on `.original`, not chained, so tracebacks and loggers
    don't print its message (it may hold bodies or keys)."""
    if isinstance(e, LetsDecideError):
        if isinstance(e.details, dict):
            e.details.setdefault("provider", name)
        return e
    err = LetsDecideError(
        "ProviderError", f"custom provider `{name}` threw {type(e).__name__}"
    )
    err.provider = name
    err.original = e
    return err


def _invalid(message, **details):
    return LetsDecideError("InvalidRequest", message, details=details or None)


def _accepts_decide(fn):
    """Whether an engine's `decide` takes the `decide=` callback."""
    try:
        params = inspect.signature(fn).parameters.values()
    except (TypeError, ValueError):
        return False
    return any(p.name == "decide" or p.kind is p.VAR_KEYWORD for p in params)


class _Custom:
    def __init__(self, kind, impl):
        self.kind = kind
        self.impl = impl
        self.capabilities = getattr(impl, "capabilities", None)
        if callable(self.capabilities):
            self.capabilities = self.capabilities()
        self.wants_decide = kind == "engine" and _accepts_decide(impl.decide)


class _Base:
    def __init__(
        self,
        *,
        by_provider=None,
        headers=None,
        providers=None,
        engines=None,
        dialects=None,
        replace=(),
        **unknown,
    ):
        """`by_provider`: settings per provider prefix, e.g. `{"gemini":
        {"api_key": ...}, "clef": {"access_token": ..., "account_id": ...}}`
        (fields: api_key, access_token, base_url, account_id, headers,
        options). A provider only ever sees its own entry; any value may be a
        callable, called per request (e.g. to refresh an OAuth token).
        `headers`: sent to every provider (tracing, gateways); never secrets.
        `providers`: custom HTTP codecs by name (`build`, `parse`, optional
        `capabilities`). `engines`: custom engines by name (`decide`, optional
        `capabilities`; a `decide=` keyword receives a callback to call other
        models). `dialects`: providers described as data, e.g. `{"acme":
        {"extends": "jev", "base_url": ..., "path": ...}}` (see
        docs/dialects.md); validated here. `replace`: built-in names they may
        override (e.g. a patched `gemini` dialect)."""
        for k in unknown:
            hint = (
                f"set it per provider: by_provider[<name>][{k!r}]"
                if k in _FIELDS
                else "unknown setting"
            )
            raise _invalid(f"config `{k}`: {hint}", reason="invalid_config", field=k)
        self._shared_headers = headers
        self._by_provider = by_provider or {}
        for name, own in self._by_provider.items():
            if set(own) - set(_FIELDS):
                raise _invalid(
                    f"by_provider[{name!r}]: unknown setting",
                    reason="invalid_config",
                    field="by_provider",
                    provider=name,
                )
        self._custom = {}
        tables = [("codec", providers or {}), ("engine", engines or {}), ("dialect", dialects or {})]
        for kind, table in tables:
            for name, impl in table.items():
                if not _NAME.fullmatch(name):
                    raise _invalid(
                        "provider names must match `[a-z0-9_-]{1,64}`",
                        reason="invalid_config",
                        field="name",
                    )
                if name in self._custom or (name in BUILTIN and name not in replace):
                    raise _invalid(
                        f"provider `{name}` is already registered; list it in `replace` to override it",
                        reason="name_taken",
                        provider=name,
                    )
                if kind == "dialect":
                    try:
                        impl = _DialectCodec(impl)
                    except LetsDecideError as e:
                        raise _from_user_code(name, e) from None
                    kind = "codec"
                needs = ("build", "parse") if kind == "codec" else ("decide",)
                for fn in needs:
                    if not callable(getattr(impl, fn, None)):
                        raise TypeError(f"{kind} `{name}` needs a `{fn}` method")
                self._custom[name] = _Custom(kind, impl)

    def _raw(self, name):
        """This provider's own settings, plus the shared headers."""
        own = self._by_provider.get(name, {})
        raw = {k: own.get(k) for k in _FIELDS}
        raw["shared_headers"] = self._shared_headers
        return raw

    @staticmethod
    def _finish(cfg):
        shared = _pairs(cfg.pop("shared_headers"))
        mine = _pairs(cfg["headers"])
        # The provider's own headers win over shared ones with the same name.
        names = {k.lower() for k, _ in mine}
        cfg["headers"] = [h for h in shared if h[0].lower() not in names] + mine
        cfg["options"] = dict(cfg["options"] or {})
        return cfg

    def _settings(self, name):
        """Resolved settings for one provider (callables called)."""
        out = {}
        for k, v in self._raw(name).items():
            v = v() if callable(v) else v
            if inspect.isawaitable(v):
                v.close()
                raise TypeError(f"`{k}` returned an awaitable: use AsyncClient")
            out[k] = v
        return self._finish(out)

    async def _asettings(self, name):
        out = {}
        for k, v in self._raw(name).items():
            v = v() if callable(v) else v
            out[k] = await v if inspect.isawaitable(v) else v
        return self._finish(out)

    def capabilities(self, model: str) -> dict:
        """Capabilities of any `"provider/model"`, built-in or custom."""
        custom = self._custom.get(model.partition("/")[0])
        if custom is not None:
            return custom.capabilities or {}
        return capabilities(model)

    def _custom_for(self, model):
        name, _, rest = model.partition("/")
        custom = self._custom.get(name)
        if custom is None:
            return None
        if not rest:
            raise _invalid("model must look like `provider/model`")
        return name, rest, custom

    @staticmethod
    def _pre(name, custom, request):
        try:
            validate(request, custom.capabilities)
        except LetsDecideError as e:
            raise _from_user_code(name, e) from None

    @staticmethod
    def _http(cfg, http):
        """Normalises a codec's request and adds `headers` from config."""
        extra = cfg["headers"]
        headers = [
            h
            for h in _pairs(http.get("headers"))
            if not any(h[0].lower() == k.lower() for k, _ in extra)
        ]
        for k, v in extra:
            if not _HEADER_NAME.fullmatch(k) or any(c in v for c in "\r\n\0"):
                raise _invalid(
                    "a header in `headers` is not valid", reason="invalid_config", field="headers"
                )
            headers.append([k, v])
        return {
            "method": http.get("method", "POST"),
            "url": http["url"],
            "headers": headers,
            "body": http.get("body", ""),
        }

    @staticmethod
    def _post(name, request, out):
        try:
            return check(request, out)
        except LetsDecideError as e:
            raise _from_user_code(name, e) from None

    @staticmethod
    def _build(model, request, cfg):
        body = json.dumps(request)
        http = _unwrap(_core.build(model, body, json.dumps(cfg)))
        return body, http

    @staticmethod
    def _too_deep(depth):
        if depth >= MAX_DEPTH:
            raise _invalid(
                f"engines nested more than {MAX_DEPTH} deep (does an engine call itself?)",
                reason="invalid_config",
                field="engines",
            )

    @staticmethod
    def _parse(model, body, res):
        reply = json.dumps(
            {"status": res.status_code, "headers": list(res.headers.items()), "body": res.text}
        )
        return _unwrap(_core.parse(model, body, reply))


def _reply(res):
    return {"status": res.status_code, "headers": list(res.headers.items()), "body": res.text}


def _send_args(http):
    return http["method"], http["url"], {"headers": dict(http["headers"]), "content": http["body"]}


def _call(name, fn, *args):
    try:
        return fn(*args)
    except Exception as e:  # noqa: BLE001 - user code: anything can come out
        raise _from_user_code(name, e) from None


async def _acall(name, fn, *args):
    try:
        out = fn(*args)
        return await out if inspect.isawaitable(out) else out
    except Exception as e:  # noqa: BLE001
        raise _from_user_code(name, e) from None


class Client(_Base):
    """Sync client. `client`: any `httpx.Client` (proxies, transport, timeouts)."""

    def __init__(self, *, client=None, **config):
        super().__init__(**config)
        self._client = client

    @property
    def _http_client(self):
        # Created lazily so engine-only clients don't need httpx.
        if self._client is None:
            self._client = _httpx().Client(timeout=_TIMEOUT)
        return self._client

    def decide(self, model: str, request: dict, *, _depth: int = 0) -> dict:
        """`model`: "provider/model". `request`: `{input, questions, media?}`."""
        custom = self._custom_for(model)
        if custom:
            return self._decide_custom(*custom, request, _depth)
        cfg = self._settings(model.partition("/")[0])
        body, http = self._build(model, request, cfg)
        method, url, kw = _send_args(http)
        return self._parse(model, body, self._http_client.request(method, url, **kw))

    def _decide_custom(self, name, model, custom, request, depth):
        self._pre(name, custom, request)
        impl = custom.impl
        if custom.kind == "engine":
            self._too_deep(depth)
            kw = {}
            if custom.wants_decide:
                kw["decide"] = lambda m, r: self.decide(m, r, _depth=depth + 1)
            out = _call(name, lambda: impl.decide(model, request, **kw))
            if inspect.isawaitable(out):
                out.close()
                raise TypeError(f"engine `{name}` is async: use AsyncClient")
        else:
            cfg = self._settings(name)
            http = self._http(cfg, _call(name, impl.build, model, request, cfg))
            method, url, kw = _send_args(http)
            res = self._http_client.request(method, url, **kw)
            out = _call(name, impl.parse, request, _reply(res))
        return self._post(name, request, out)


class AsyncClient(_Base):
    """Async client. `client`: any `httpx.AsyncClient`."""

    def __init__(self, *, client=None, **config):
        super().__init__(**config)
        self._client = client

    @property
    def _http_client(self):
        if self._client is None:
            self._client = _httpx().AsyncClient(timeout=_TIMEOUT)
        return self._client

    async def decide(self, model: str, request: dict, *, _depth: int = 0) -> dict:
        custom = self._custom_for(model)
        if custom:
            return await self._decide_custom(*custom, request, _depth)
        cfg = await self._asettings(model.partition("/")[0])
        body, http = self._build(model, request, cfg)
        method, url, kw = _send_args(http)
        return self._parse(model, body, await self._http_client.request(method, url, **kw))

    async def _decide_custom(self, name, model, custom, request, depth):
        self._pre(name, custom, request)
        impl = custom.impl
        if custom.kind == "engine":
            self._too_deep(depth)
            kw = {}
            if custom.wants_decide:

                async def decide(m, r):
                    return await self.decide(m, r, _depth=depth + 1)

                kw["decide"] = decide
            out = await _acall(name, lambda: impl.decide(model, request, **kw))
        else:
            cfg = await self._asettings(name)
            http = self._http(cfg, await _acall(name, impl.build, model, request, cfg))
            method, url, kw = _send_args(http)
            res = await self._http_client.request(method, url, **kw)
            out = await _acall(name, impl.parse, request, _reply(res))
        return self._post(name, request, out)


class _Mock:
    capabilities = {"media": ["image/*", "audio/*", "video/*", "application/pdf"]}

    def __init__(self, answers, model, probability_source):
        self._answers = answers
        self._model = model
        self._source = probability_source

    def decide(self, model, request):
        out = {}
        for qid, q in request["questions"].items():
            if qid in self._answers:
                out[qid] = self._answers[qid]
            elif q["kind"] == "yesno":
                out[qid] = {"kind": "yesno", "probability": 0.5}
            else:
                if q["kind"] == "choice":
                    ids = sorted(q["options"])
                else:
                    ids = [lv["id"] for lv in q["levels"]]
                p = 1 / len(ids)
                probs = {i: p for i in ids}
                if q["kind"] == "choice":
                    out[qid] = {"kind": "choice", "choice": ids[0], "confidence": p, "probabilities": probs}
                else:
                    value = (len(ids) - 1) / 2
                    out[qid] = {"kind": "score", "value": value, "confidence": p, "probabilities": probs}
        return {
            "model": self._model,
            "answers": out,
            "input_tokens": None,
            "probability_source": self._source,
        }


def mock_engine(answers=None, model="mock", probability_source="provider"):
    """A deterministic engine for tests: no network, no keys. Answers each
    question with `answers[id]` if given, else a neutral answer (yes/no 0.5,
    first option, middle level). Works with `Client` and `AsyncClient`."""
    return _Mock(answers or {}, model, probability_source)
