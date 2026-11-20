"""One request shape for every decision API. See README.md."""

from __future__ import annotations

import inspect
import json
import re
from typing import Any

from . import _core

__all__ = [
    "BUILTIN",
    "AsyncClient",
    "Client",
    "LetsDecideError",
    "capabilities",
    "check",
    "classify_http",
    "mock_engine",
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


class _Custom:
    def __init__(self, kind, impl):
        self.kind = kind
        self.impl = impl
        self.capabilities = getattr(impl, "capabilities", None)
        if callable(self.capabilities):
            self.capabilities = self.capabilities()


class _Base:
    def __init__(
        self,
        *,
        api_key=None,
        access_token=None,
        base_url=None,
        account_id=None,
        headers=None,
        options=None,
        providers=None,
        engines=None,
        replace=(),
    ):
        """`access_token`: OAuth token (Clef) or a callable returning a fresh one,
        called on every request. Wins over `api_key`. `headers`: dict or list of
        pairs, sent last (replace same-named ones). `options`: free-form
        settings for custom providers. `providers`: custom HTTP codecs by name
        (`build`, `parse`, optional `capabilities`). `engines`: custom engines by
        name (`decide`, optional `capabilities`). `replace`: built-in names they
        may override."""
        self._api_key = api_key
        self._access_token = access_token
        self._base_url = base_url
        self._account_id = account_id
        self._headers = _pairs(headers)
        self._options = options or {}
        self._custom = {}
        for kind, table in (("codec", providers or {}), ("engine", engines or {})):
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
                needs = ("build", "parse") if kind == "codec" else ("decide",)
                for fn in needs:
                    if not callable(getattr(impl, fn, None)):
                        raise TypeError(f"{kind} `{name}` needs a `{fn}` method")
                self._custom[name] = _Custom(kind, impl)

    def _config(self, token):
        return json.dumps(
            {
                "api_key": self._api_key,
                "access_token": token,
                "base_url": self._base_url,
                "account_id": self._account_id,
                "headers": self._headers,
                "options": self._options,
            }
        )

    def _provider_config(self, token):
        """Config as a custom codec sees it."""
        return {
            "api_key": self._api_key,
            "access_token": token,
            "base_url": self._base_url,
            "account_id": self._account_id,
            "headers": [list(h) for h in self._headers],
            "options": dict(self._options),
        }

    def _token(self):
        t = self._access_token
        return t() if callable(t) else t

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

    def _http(self, name, http):
        """Normalises a codec's request and adds `headers` from config."""
        headers = [
            h
            for h in _pairs(http.get("headers"))
            if not any(h[0].lower() == k.lower() for k, _ in self._headers)
        ]
        for k, v in self._headers:
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

    def _build(self, model, request, token):
        body = json.dumps(request)
        http = _unwrap(_core.build(model, body, self._config(token)))
        return body, http

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

    def decide(self, model: str, request: dict) -> dict:
        """`model`: "provider/model". `request`: `{input, questions, media?}`."""
        custom = self._custom_for(model)
        if custom:
            return self._decide_custom(*custom, request)
        body, http = self._build(model, request, self._token())
        method, url, kw = _send_args(http)
        return self._parse(model, body, self._http_client.request(method, url, **kw))

    def _decide_custom(self, name, model, custom, request):
        self._pre(name, custom, request)
        impl = custom.impl
        if custom.kind == "engine":
            out = _call(name, impl.decide, model, request)
            if inspect.isawaitable(out):
                out.close()
                raise TypeError(f"engine `{name}` is async: use AsyncClient")
        else:
            cfg = self._provider_config(self._token())
            http = self._http(name, _call(name, impl.build, model, request, cfg))
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

    async def _atoken(self):
        token = self._token()
        return await token if inspect.isawaitable(token) else token

    async def decide(self, model: str, request: dict) -> dict:
        custom = self._custom_for(model)
        if custom:
            return await self._decide_custom(*custom, request)
        body, http = self._build(model, request, await self._atoken())
        method, url, kw = _send_args(http)
        return self._parse(model, body, await self._http_client.request(method, url, **kw))

    async def _decide_custom(self, name, model, custom, request):
        self._pre(name, custom, request)
        impl = custom.impl
        if custom.kind == "engine":
            out = await _acall(name, impl.decide, model, request)
        else:
            cfg = self._provider_config(await self._atoken())
            http = self._http(name, await _acall(name, impl.build, model, request, cfg))
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
