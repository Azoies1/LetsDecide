"""One request shape for every decision API. See README.md."""

import inspect
import json
from typing import Any

from . import _core

__all__ = ["Client", "AsyncClient", "LetsDecideError"]


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
    ):
        """`access_token`: OAuth token (Clef) or a callable returning a fresh one,
        called on every request. Wins over `api_key`. `headers`: dict or list of
        pairs, sent last (replace same-named ones). `options`: free-form
        settings for custom providers."""
        headers = headers or []
        if isinstance(headers, dict):
            headers = list(headers.items())
        self._api_key = api_key
        self._access_token = access_token
        self._base_url = base_url
        self._account_id = account_id
        self._headers = [list(h) for h in headers]
        self._options = options or {}

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

    def _token(self):
        t = self._access_token
        return t() if callable(t) else t

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


def _send_args(http):
    return http["method"], http["url"], {"headers": dict(http["headers"]), "content": http["body"]}


class Client(_Base):
    """Sync client. `client`: any `httpx.Client` (proxies, transport, timeouts)."""

    def __init__(self, *, client=None, **config):
        super().__init__(**config)
        self._http = client or _httpx().Client(timeout=_TIMEOUT)

    def decide(self, model: str, request: dict) -> dict:
        """`model`: "provider/model". `request`: `{input, questions, media?}`."""
        body, http = self._build(model, request, self._token())
        method, url, kw = _send_args(http)
        return self._parse(model, body, self._http.request(method, url, **kw))


class AsyncClient(_Base):
    """Async client. `client`: any `httpx.AsyncClient`."""

    def __init__(self, *, client=None, **config):
        super().__init__(**config)
        self._http = client or _httpx().AsyncClient(timeout=_TIMEOUT)

    async def decide(self, model: str, request: dict) -> dict:
        token = self._token()
        if inspect.isawaitable(token):
            token = await token
        body, http = self._build(model, request, token)
        method, url, kw = _send_args(http)
        return self._parse(model, body, await self._http.request(method, url, **kw))
