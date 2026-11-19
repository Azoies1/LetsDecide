"""One request shape for every decision API. See README.md."""

import json
from typing import Any

from . import _core

__all__ = ["Client", "AsyncClient", "LetsDecideError"]


class LetsDecideError(Exception):
    def __init__(self, code, message, status=None, retry_after_ms=None, retryable=False):
        super().__init__(message)
        self.code = code
        self.status = status
        self.retry_after_ms = retry_after_ms
        self.retryable = retryable


def _unwrap(text: str) -> Any:
    """The core returns {ok} or {error}; unwrap or raise."""
    out = json.loads(text)
    if "error" in out:
        raise LetsDecideError(**out["error"])
    return out["ok"]


def _httpx():
    try:
        import httpx
    except ImportError as e:
        raise ImportError("install `letsdecide[httpx]` or pass your own client") from e
    return httpx


class _Base:
    def __init__(self, *, api_key=None, base_url=None, account_id=None):
        self._config = json.dumps(
            {"api_key": api_key, "base_url": base_url, "account_id": account_id}
        )

    def _build(self, model, request):
        body = json.dumps(request)
        http = _unwrap(_core.build(model, body, self._config))
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
        self._http = client or _httpx().Client()

    def decide(self, model: str, request: dict) -> dict:
        """`model`: "provider/model". `request`: `{input, questions, images?}`."""
        body, http = self._build(model, request)
        method, url, kw = _send_args(http)
        return self._parse(model, body, self._http.request(method, url, **kw))


class AsyncClient(_Base):
    """Async client. `client`: any `httpx.AsyncClient`."""

    def __init__(self, *, client=None, **config):
        super().__init__(**config)
        self._http = client or _httpx().AsyncClient()

    async def decide(self, model: str, request: dict) -> dict:
        body, http = self._build(model, request)
        method, url, kw = _send_args(http)
        return self._parse(model, body, await self._http.request(method, url, **kw))
