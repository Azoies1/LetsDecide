import asyncio

import httpx
import pytest

from letsdecide import AsyncClient, LetsDecideError
from letsdecide.middleware import (
    brier_score,
    calibrate,
    compare,
    compose,
    fallback,
    fit_isotonic,
    hedge,
    observe,
    retry,
    shadow,
    split,
)


def err(code, retryable=False):
    return LetsDecideError(code, code, retryable=retryable)


def yes(p):
    return {"kind": "yesno", "probability": p}


def qs(n):
    return {f"q{i}": {"kind": "yesno", "instructions": "?"} for i in range(n)}


class Fake:
    """`impl(model, request, n)` is an async function; calls are recorded."""

    def __init__(self, impl):
        self.impl = impl
        self.calls = []

    async def decide(self, model, request):
        self.calls.append((model, request))
        return await self.impl(model, request, len(self.calls))


async def answer_all(model, request, n=0):
    return {"model": model, "answers": {q: yes(0.9) for q in request["questions"]}, "input_tokens": 10, "estimated_cost_usd": 0.5}


async def no_sleep(_s):
    pass


# --- retry and fallback, end to end through the real client ---

OK = {"answers": {"q": {"type": "noul", "noul": 0.7}}}
REQ = {"input": "x", "questions": {"q": {"kind": "yesno", "instructions": "?"}}}


def scripted(script):
    urls = []

    def handler(req):
        urls.append(str(req.url))
        status, body, headers = (script.pop(0) + [{}, {}])[:3]
        return httpx.Response(status, json=body, headers=headers)

    return urls, AsyncClient(client=httpx.AsyncClient(transport=httpx.MockTransport(handler)),
        by_provider={
            "jev": {"api_key": "k"},
            "openai": {"api_key": "k"},
            "gemini": {"api_key": "k"},
            "clef": {"api_key": "k", "account_id": "a"},
        },
    )


async def test_retry_then_succeed_and_retry_after():
    urls, c = scripted([[503], [429, {}, {"retry-after": "2"}], [200, OK]])
    waits = []

    async def sleep(s):
        waits.append(s)

    res = await compose(c, retry(sleep=sleep))("jev/jev-latest", REQ)
    assert res["attempts"] == 3
    assert len(urls) == 3
    assert 0.1 <= waits[0] <= 0.2 and waits[1] == 2


async def test_retry_stops_on_hard_errors_and_exhaustion():
    _, c = scripted([[401]])
    with pytest.raises(LetsDecideError) as e:
        await compose(c, retry(sleep=no_sleep))("jev/jev-latest", REQ)
    assert e.value.code == "Auth" and e.value.attempts == 1
    _, c = scripted([[503], [503]])
    with pytest.raises(LetsDecideError) as e:
        await compose(c, retry(sleep=no_sleep, max_attempts=2))("jev/jev-latest", REQ)
    assert e.value.attempts == 2


async def test_fallback_on_5xx_and_unsupported():
    urls, c = scripted([[503], [200, OK]])
    res = await compose(c, fallback(["gemini/gemini-decision-1"]))("jev/jev-latest", REQ)
    assert res["fell_back_from"] == "jev/jev-latest" and res["attempts"] == 2
    assert "googleapis" in urls[1]
    _, c = scripted([[200, OK]])
    res = await compose(c, fallback(["gemini/gemini-decision-1"]))("nope/model", REQ)
    assert res["fell_back_from"] == "nope/model"
    urls, c = scripted([[401]])
    with pytest.raises(LetsDecideError):
        await compose(c, fallback(["openai/gpt-6-luna"]))("jev/jev-latest", REQ)
    assert len(urls) == 1


# --- hedge ---


async def test_hedge_slow_call_is_overtaken_and_cancelled():
    cancelled = []

    async def impl(model, req, n):
        try:
            await asyncio.sleep(1 if n == 1 else 0.005)
        except asyncio.CancelledError:
            cancelled.append(n)
            raise
        return await answer_all(model, req)

    f = Fake(impl)
    res = await compose(f, hedge(delay_ms=20, models=["gemini/b"]))("jev/a", {"questions": qs(1)})
    assert res["model"] == "gemini/b" and res["hedged_from"] == "jev/a"
    await asyncio.sleep(0)
    assert cancelled == [1]


async def test_hedge_fast_answer_and_failures():
    f = Fake(answer_all)
    res = await compose(f, hedge(delay_ms=50))("jev/a", {"questions": qs(1)})
    assert "hedged_from" not in res and len(f.calls) == 1

    async def flaky(model, req, n):
        if n == 1:
            raise err("ProviderUnavailable", True)
        return await answer_all(model, req)

    res = await compose(Fake(flaky), hedge(delay_ms=10_000))("jev/a", {"questions": qs(1)})
    assert res["hedged_from"] == "jev/a"

    async def auth(*_):
        raise err("Auth")

    f = Fake(auth)
    with pytest.raises(LetsDecideError) as e:
        await compose(f, hedge(delay_ms=10_000))("jev/a", {"questions": qs(1)})
    assert e.value.code == "Auth" and len(f.calls) == 1

    async def limited(*_):
        raise err("RateLimited", True)

    f = Fake(limited)
    with pytest.raises(LetsDecideError) as e:
        await compose(f, hedge(delay_ms=5))("jev/a", {"questions": qs(1)})
    assert e.value.code == "RateLimited" and len(f.calls) == 2


# --- split ---


async def test_split_chunks_and_merges():
    f = Fake(answer_all)
    res = await compose(f, split())("clef/clef-flash", {"input": "x", "questions": qs(130)})
    assert [len(r["questions"]) for _, r in f.calls] == [64, 64, 2]
    assert len(res["answers"]) == 130 and res["input_tokens"] == 30 and res["parts"] == 3 and res["estimated_cost_usd"] == 1.5

    f = Fake(answer_all)
    res = await compose(f, split())("jev/x", {"questions": qs(500)})
    assert len(f.calls) == 1 and "parts" not in res

    f = Fake(answer_all)
    await compose(f, split(max_questions=lambda m: 2))("jev/x", {"questions": qs(5)})
    assert len(f.calls) == 3

    async def second_fails(model, req, n):
        if n == 2:
            raise err("Auth")
        return await answer_all(model, req)

    with pytest.raises(LetsDecideError):
        await compose(Fake(second_fails), split(max_questions=1, concurrency=1))("jev/x", {"questions": qs(3)})


# --- shadow and compare ---


async def test_shadow_reports_and_never_raises():
    reports = []

    async def impl(model, req, n):
        if model == "gemini/b":
            return {"model": model, "answers": {"q0": yes(0.2)}, "input_tokens": 1}
        return await answer_all(model, req)

    res = await compose(Fake(impl), shadow(model="gemini/b", on_result=reports.append))("jev/a", {"questions": qs(1)})
    assert res["model"] == "jev/a"
    await asyncio.sleep(0.01)
    assert reports[0]["shadow"]["model"] == "gemini/b"
    assert reports[0]["comparison"]["agreement"] == 0

    async def shadow_fails(model, req, n):
        if model == "gemini/b":
            raise err("Auth")
        return await answer_all(model, req)

    res = await compose(Fake(shadow_fails), shadow(model="gemini/b", on_result=reports.append))("jev/a", {"questions": qs(1)})
    assert res["model"] == "jev/a"
    await asyncio.sleep(0.01)
    assert reports[1]["error"].code == "Auth"

    f = Fake(answer_all)
    await compose(f, shadow(model="gemini/b", on_result=reports.append, sample=0.5, random=lambda: 0.9))(
        "jev/a", {"questions": qs(1)}
    )
    assert len(f.calls) == 1


def test_compare():
    a = {"answers": {"y": yes(0.8), "c": {"kind": "choice", "choice": "a"}, "s": {"kind": "score", "value": 1.2}, "r": {"kind": "refused"}}}
    b = {"answers": {"y": yes(0.6), "c": {"kind": "choice", "choice": "b"}, "s": {"kind": "score", "value": 0.9}, "r": {"kind": "refused"}}}
    out = compare(a, b)
    assert out["questions"]["y"]["agree"] and abs(out["questions"]["y"]["delta"] - 0.2) < 1e-9
    assert not out["questions"]["c"]["agree"]
    assert out["questions"]["s"]["agree"]
    assert "r" not in out["questions"]
    assert out["agreement"] == 2 / 3


# --- calibration ---


async def test_calibrate_keeps_raw():
    async def impl(*_):
        return {
            "answers": {
                "y": yes(0.8),
                "c": {"kind": "choice", "choice": "a", "confidence": 0.6, "probabilities": {"a": 0.6, "b": 0.4}},
                "r": {"kind": "refused"},
            }
        }

    res = await compose(Fake(impl), calibrate(lambda p: p / 2))("jev/a", {})
    assert res["answers"]["y"]["probability"] == 0.8
    assert res["answers"]["y"]["calibrated"] == {"probability": 0.4}
    assert res["answers"]["c"]["calibrated"]["probabilities"] == pytest.approx({"a": 0.6, "b": 0.4})
    assert "calibrated" not in res["answers"]["r"]
    res = await compose(Fake(impl), calibrate({"y": lambda p: 2}))("jev/a", {})
    assert res["answers"]["y"]["calibrated"]["probability"] == 1
    assert "calibrated" not in res["answers"]["c"]


def test_fit_isotonic_and_brier():
    samples = [s for i in range(50) for s in ((0.9, i % 2), (0.1, 0))]
    f = fit_isotonic(samples)
    assert f(0.9) == pytest.approx(0.5) and f(0.1) == 0
    assert f(0.1) <= f(0.5) <= f(0.9)
    assert brier_score([(f(p), y) for p, y in samples]) < brier_score(samples)
    assert brier_score([]) is None


# --- observe ---


async def test_observe_hooks():
    seen = []
    t = iter(range(0, 100))

    def on_end(e):
        seen.append(("end", e["duration_ms"]))
        raise RuntimeError("hook bug")

    hooks = dict(
        now=lambda: next(t) * 0.005,
        on_start=lambda e: seen.append(("start", e["model"])),
        on_end=on_end,
        on_error=lambda e: seen.append(("error", e["error"].code)),
    )
    res = await compose(Fake(answer_all), observe(**hooks))("jev/a", {"questions": qs(1)})
    assert res["model"] == "jev/a"

    async def auth(*_):
        raise err("Auth")

    with pytest.raises(LetsDecideError):
        await compose(Fake(auth), observe(**hooks))("jev/a", {})
    assert seen[0] == ("start", "jev/a") and seen[1][0] == "end" and seen[1][1] == pytest.approx(5)
    assert seen[3] == ("error", "Auth")



# --- review fixes ---


async def test_retry_after_zero_means_now():
    waits = []

    async def sleep(s):
        waits.append(s)

    async def impl(model, req, n):
        if n == 1:
            raise LetsDecideError("RateLimited", "x", retry_after_ms=0, retryable=True)
        return await answer_all(model, req)

    await compose(Fake(impl), retry(sleep=sleep))("jev/a", {"questions": qs(1)})
    assert waits == [0]


async def test_hedge_hard_error_waits_for_call_in_flight():
    async def impl(model, req, n):
        if n == 2:
            raise err("InvalidRequest")
        await asyncio.sleep(0.04)
        return await answer_all(model, req)

    f = Fake(impl)
    res = await compose(f, hedge(delay_ms=10, models=["gemini/b", "openai/c"]))("clef/a", {"questions": qs(1)})
    assert res["model"] == "clef/a" and len(f.calls) == 2

    async def worse(model, req, n):
        if n == 2:
            raise err("InvalidRequest")
        await asyncio.sleep(0.02)
        raise err("ProviderUnavailable", True)

    with pytest.raises(LetsDecideError) as e:
        await compose(Fake(worse), hedge(delay_ms=5, models=["gemini/b"]))("clef/a", {"questions": qs(1)})
    assert e.value.code == "InvalidRequest"


async def test_split_merge_and_bad_options():
    async def impl(_m, req, n):
        out = {
            "model": "clef-flash" if n == 1 else "jev-1",
            "answers": {q: ({"kind": "refused"} if n == 1 else yes(0.5)) for q in req["questions"]},
            "input_tokens": 1,
        }
        if n == 1:
            out["block_reason"] = "SAFETY"
        else:
            out["fell_back_from"] = "clef/clef-flash"
        return out

    res = await compose(Fake(impl), split(max_questions=1, concurrency=1))("clef/clef-flash", {"questions": qs(2)})
    assert res["models"] == ["clef-flash", "jev-1"]
    assert res["fell_back_from"] == "clef/clef-flash"
    assert "block_reason" not in res
    with pytest.raises(ValueError):
        split(concurrency=0)
    with pytest.raises(ValueError):
        await compose(Fake(answer_all), split(max_questions=0))("jev/x", {"questions": qs(3)})


def test_compare_rounds_like_js():
    a = {"answers": {"s": {"kind": "score", "value": -2.5}}}
    b = {"answers": {"s": {"kind": "score", "value": -2.0}}}
    assert compare(a, b)["questions"]["s"]["agree"]
