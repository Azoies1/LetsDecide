"""Conformance kit: run a custom provider against fixtures in the same format
as the core's golden fixtures (`tests/fixtures/**.json`).

Fixture: ``{name?, model, config?, request, http?, response?, expected}``

- codecs: ``http`` is the exact request to build (``{url, headers, body}``)
  and ``response`` the recorded reply (``{status, headers, body}``).
- engines: no ``http`` or ``response``; ``decide`` is called directly.
- ``expected``: ``{"ok": response}`` or ``{"error": {"code", "retry_after_ms"?}}``.

    report = check_provider(MyCodec(), fixtures)
    assert report["failed"] == 0, report["results"]
"""

from __future__ import annotations

import asyncio
import inspect
import json

from . import LetsDecideError, check

__all__ = ["check_provider"]


def _canon(v):
    return json.dumps(v, sort_keys=True)


def _headers(h):
    items = h.items() if isinstance(h, dict) else (h or [])
    return {k.lower(): v for k, v in items}


def _run(out):
    return asyncio.run(out) if inspect.isawaitable(out) else out


def check_provider(impl, fixtures) -> dict:
    """Runs ``impl`` (a codec with ``build``/``parse`` or an engine with
    ``decide``) against ``fixtures``. Returns ``{passed, failed, results}``,
    each result ``{name, ok, problems}``. A failing fixture never raises."""
    results = []
    for i, fx in enumerate(fixtures):
        name = fx.get("name", f"fixture {i}")
        problems = []
        model = fx["model"].partition("/")[2]
        want = fx["expected"]
        try:
            if callable(getattr(impl, "decide", None)):
                got = _run(impl.decide(model, fx["request"]))
            else:
                cfg = dict(fx.get("config") or {})
                cfg.setdefault("headers", [])
                cfg.setdefault("options", {})
                http = _run(impl.build(model, fx["request"], cfg))
                if "http" in fx:
                    if http["url"] != fx["http"]["url"]:
                        problems.append(f"url: got {http['url']}, want {fx['http']['url']}")
                    if _headers(http.get("headers")) != _headers(fx["http"]["headers"]):
                        problems.append("headers differ")
                    body = http.get("body", "")
                    try:
                        body = json.loads(body) if isinstance(body, (str, bytes)) else body
                    except ValueError:
                        problems.append("body is not JSON")
                    if _canon(body) != _canon(fx["http"]["body"]):
                        problems.append("body differs")
                r = fx["response"]
                body = r["body"] if isinstance(r["body"], str) else json.dumps(r["body"])
                reply = {"status": r["status"], "headers": r.get("headers", []), "body": body}
                got = _run(impl.parse(fx["request"], reply))
            got = check(fx["request"], got)
            if "error" in want:
                problems.append(f"expected error {want['error']['code']}, got a response")
            elif _canon(got) != _canon(check(fx["request"], want["ok"])):
                problems.append("response differs from expected")
        except LetsDecideError as e:
            err = want.get("error")
            if not err:
                problems.append(f"unexpected {e.code}: {e}")
            else:
                if e.code != err["code"]:
                    problems.append(f"error code: got {e.code}, want {err['code']}")
                if "retry_after_ms" in err and e.retry_after_ms != err["retry_after_ms"]:
                    problems.append(
                        f"retry_after_ms: got {e.retry_after_ms}, want {err['retry_after_ms']}"
                    )
        except Exception as e:  # noqa: BLE001 - report, don't crash the run
            problems.append(f"raised {type(e).__name__} instead of LetsDecideError")
        results.append({"name": name, "ok": not problems, "problems": problems})
    passed = sum(r["ok"] for r in results)
    return {"passed": passed, "failed": len(results) - passed, "results": results}
