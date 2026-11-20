"""Opt-in middleware for `AsyncClient`, the same set as the JS binding.

A middleware is `(next) -> decide`, where `decide(model, request)` is an async
function returning a response dict. `AsyncClient.decide` is the base `next`.

    decide = compose(client, fallback(["jev/jev-latest"]), retry())
    out = await decide("clef/clef-flash", request)
"""

from __future__ import annotations

import asyncio
import math
import random as _random
import time
from typing import Any, Awaitable, Callable, Mapping

Decide = Callable[[str, dict], Awaitable[dict]]
Middleware = Callable[[Decide], Decide]
Calibrator = Callable[[float], float]

__all__ = [
    "compose",
    "retry",
    "fallback",
    "hedge",
    "split",
    "shadow",
    "compare",
    "calibrate",
    "fit_isotonic",
    "brier_score",
    "observe",
    "fan_out",
]


def compose(client: Any, *middleware: Middleware) -> Decide:
    """Wraps `client.decide` (async) in the middleware; the first is outermost."""
    decide: Decide = client.decide
    for mw in reversed(middleware):
        decide = mw(decide)
    return decide


def _recoverable(e: BaseException) -> bool:
    return getattr(e, "retryable", False) or getattr(e, "code", None) == "Unsupported"


def retry(*, max_attempts=3, base_ms=200, max_ms=30_000, sleep=asyncio.sleep) -> Middleware:
    """Retries `retryable` errors with jittered exponential backoff, honouring
    `retry_after_ms` (capped at `max_ms`). `sleep` takes seconds."""

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            attempts = 0
            while True:
                attempts += 1
                try:
                    return {**await next(model, request), "attempts": attempts}
                except Exception as e:
                    if not getattr(e, "retryable", False) or attempts >= max_attempts:
                        e.attempts = attempts
                        raise
                    backoff = base_ms * 2 ** (attempts - 1) * (0.5 + _random.random() / 2)
                    after = getattr(e, "retry_after_ms", None)
                    wait = backoff if after is None else after  # 0 means "now"
                    await sleep(min(wait, max_ms) / 1000)

        return decide

    return mw


def fallback(models: list[str]) -> Middleware:
    """On a retryable or `Unsupported` error, tries the next model."""

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            chain = [model, *models]
            attempts = 0
            for i, m in enumerate(chain):
                try:
                    res = await next(m, request)
                except Exception as e:
                    attempts += getattr(e, "attempts", 1)
                    if i + 1 >= len(chain) or not _recoverable(e):
                        e.attempts = attempts
                        raise
                    continue
                out = {**res, "attempts": attempts + res.get("attempts", 1)}
                if i:
                    out["fell_back_from"] = model
                return out

        return decide

    return mw


def hedge(*, delay_ms=1000, models: list[str] | None = None) -> Middleware:
    """Starts another call when the current one is slower than `delay_ms` (or
    fails recoverably); the first answer wins and the rest are cancelled."""

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            chain = [model, *(models or [model])]
            tasks: dict[asyncio.Task, int] = {}
            last: BaseException | None = None
            hard: BaseException | None = None  # stops new hedges; pending calls may still answer

            def launch():
                i = len(tasks)
                tasks[asyncio.ensure_future(next(chain[i], request))] = i

            launch()
            try:
                while True:
                    more = hard is None and len(tasks) < len(chain)
                    live = [t for t in tasks if not t.done()]
                    if not live:
                        if not more:
                            raise hard or last  # every call failed
                        launch()
                        continue
                    timeout = delay_ms / 1000 if more else None
                    done, _ = await asyncio.wait(live, timeout=timeout, return_when=asyncio.FIRST_COMPLETED)
                    if not done:  # too slow: hedge
                        launch()
                        continue
                    # A success wins even if a failure finished at the same time.
                    ok = [t for t in done if t.exception() is None]
                    if ok:
                        out = dict(ok[0].result())
                        if tasks[ok[0]]:
                            out["hedged_from"] = model
                        return out
                    for t in done:
                        e = t.exception()
                        if _recoverable(e):
                            last = e
                        elif hard is None:
                            hard = e
                    if hard is None and len(tasks) < len(chain):  # failed: hedge now, not after the delay
                        launch()
            finally:
                for t in tasks:
                    t.cancel()

        return decide

    return mw


QUESTION_LIMITS = {"clef": 64, "openai": 400, "gemini": 1000}


def split(*, max_questions: int | Callable[[str], int | None] | None = None, concurrency=4) -> Middleware:
    """Splits a request with more questions than the provider's cap (or
    `max_questions`) and merges the answers. Input tokens and cost are summed."""

    def limit(model: str) -> float:
        n = max_questions(model) if callable(max_questions) else max_questions
        if n is None:
            n = QUESTION_LIMITS.get(model.split("/")[0])
        if n is None:
            return float("inf")
        if n < 1:
            raise ValueError("split: max_questions must be at least 1")
        return int(n)

    if concurrency < 1:
        raise ValueError("split: concurrency must be at least 1")

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            ids = list(request["questions"])
            size = limit(model)
            if len(ids) <= size:
                return await next(model, request)
            size = int(size)
            parts = [
                {**request, "questions": {i: request["questions"][i] for i in ids[n : n + size]}}
                for n in range(0, len(ids), size)
            ]
            gate = asyncio.Semaphore(concurrency)

            async def run(part):
                async with gate:
                    return await next(model, part)

            tasks = [asyncio.ensure_future(run(p)) for p in parts]
            try:
                results = await asyncio.gather(*tasks)
            finally:
                for t in tasks:
                    t.cancel()
            return _merge(results)

        return decide

    return mw


def _merge(results: list[dict]) -> dict:
    tokens = [r.get("input_tokens") for r in results]
    out = {
        "model": next((r["model"] for r in results if r.get("model")), None),
        "answers": {k: v for r in results for k, v in r["answers"].items()},
        "input_tokens": None if None in tokens else sum(tokens),
        "parts": len(results),
    }
    costs = [r.get("estimated_cost_usd") for r in results]
    if None not in costs:
        out["estimated_cost_usd"] = sum(costs)
    # Parts may be answered by different models (fallback, hedge): say so.
    models = list(dict.fromkeys(r["model"] for r in results if r.get("model")))
    if len(models) > 1:
        out["models"] = models
    for k in ("fell_back_from", "hedged_from"):
        v = next((r[k] for r in results if r.get(k)), None)
        if v:
            out[k] = v
    # Blocked means every answer is refused, so only when every part was.
    if all(r.get("block_reason") for r in results):
        out["block_reason"] = results[0]["block_reason"]
    if any("attempts" in r for r in results):
        out["attempts"] = sum(r.get("attempts", 1) for r in results)
    return out


def shadow(*, model: str, on_result: Callable[[dict], Any], sample=1.0, random=_random.random) -> Middleware:
    """Also sends each request to `model` in the background and calls
    `on_result({model, shadow_model, request, primary, shadow, error, comparison})`.
    The caller gets the primary answer as soon as it is ready; shadow errors
    are reported, never raised."""
    shadow_model = model
    pending: set[asyncio.Task] = set()  # keeps background tasks alive

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            call = asyncio.ensure_future(next(shadow_model, request)) if random() < sample else None
            try:
                primary = await next(model, request)
            except BaseException:
                if call:
                    call.cancel()
                    # Read any shadow error so asyncio doesn't log it as unhandled.
                    call.add_done_callback(lambda t: t.cancelled() or t.exception())
                raise
            if call:

                async def report():
                    result = {"model": model, "shadow_model": shadow_model, "request": request, "primary": primary}
                    try:
                        result["shadow"] = await call
                        result["comparison"] = compare(primary, result["shadow"])
                    except Exception as e:
                        result["error"] = e
                    try:
                        r = on_result(result)
                        if asyncio.iscoroutine(r):
                            await r
                    except Exception:
                        pass  # a reporting bug must not surface on the caller's path

                task = asyncio.ensure_future(report())
                pending.add(task)
                task.add_done_callback(pending.discard)
            return primary

        return decide

    return mw


def compare(a: dict, b: dict) -> dict:
    """Per-question agreement: yes/no on the same side of 0.5, equal choice,
    or scores rounding to the same level. `delta` is the absolute difference."""
    questions = {}
    for qid, x in a["answers"].items():
        y = b["answers"].get(qid)
        if not y or x["kind"] != y["kind"] or x["kind"] == "refused":
            continue
        if x["kind"] == "yesno":
            px, py = x["probability"], y["probability"]
            questions[qid] = {"agree": (px >= 0.5) == (py >= 0.5), "delta": abs(px - py)}
        elif x["kind"] == "choice":
            questions[qid] = {"agree": x["choice"] == y["choice"]}
        else:
            questions[qid] = {
                "agree": _round(x["value"]) == _round(y["value"]),
                "delta": abs(x["value"] - y["value"]),
            }
    agree = [q["agree"] for q in questions.values()]
    return {"agreement": sum(agree) / len(agree) if agree else None, "questions": questions}


def _round(v: float) -> int:
    # Half up, as in JS `Math.round`, so both bindings agree.
    return math.floor(v + 0.5)


def calibrate(calibrators: Calibrator | Mapping[str, Calibrator]) -> Middleware:
    """Adds `calibrated` next to each raw answer; raw values are never changed."""

    def pick(qid):
        return calibrators if callable(calibrators) else calibrators.get(qid)

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            res = await next(model, request)
            answers = {}
            for qid, a in res["answers"].items():
                f = pick(qid)
                answers[qid] = {**a, "calibrated": _calibrate(a, f)} if f and a["kind"] != "refused" else a
            return {**res, "answers": answers}

        return decide

    return mw


def _calibrate(a: dict, f: Calibrator) -> dict:
    def clamp(p):
        return min(1.0, max(0.0, f(p)))

    if a["kind"] == "yesno":
        return {"probability": clamp(a["probability"])}
    raw = {k: clamp(p) for k, p in a["probabilities"].items()}
    total = sum(raw.values())
    probs = {k: (p / total if total else 0.0) for k, p in raw.items()}
    return {"probabilities": probs, "confidence": max(probs.values(), default=0.0)}


def fit_isotonic(samples: list[tuple[float, float]]) -> Calibrator:
    """Monotonic calibrator (pool-adjacent-violators) from `(predicted, outcome)`
    pairs, outcome 0 or 1; linear between fitted points."""
    points: list[list[float]] = []  # [x, sum, n]
    for x, y in sorted(samples, key=lambda s: s[0]):
        if points and points[-1][0] == x:
            points[-1][1] += y
            points[-1][2] += 1
        else:
            points.append([x, float(y), 1])
    blocks: list[list[float]] = []
    for p in points:
        blocks.append(list(p))
        while len(blocks) > 1 and blocks[-2][1] / blocks[-2][2] > blocks[-1][1] / blocks[-1][2]:
            bx, bs, bn = blocks.pop()
            a = blocks[-1]
            a[0] = (a[0] * a[2] + bx * bn) / (a[2] + bn)
            a[1] += bs
            a[2] += bn
    pts = [(x, s / n) for x, s, n in blocks]

    def f(p: float) -> float:
        if not pts:
            return p
        if p <= pts[0][0]:
            return pts[0][1]
        for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
            if p <= x1:
                return y1 if x1 == x0 else y0 + (p - x0) / (x1 - x0) * (y1 - y0)
        return pts[-1][1]

    return f


def brier_score(samples: list[tuple[float, float]]) -> float | None:
    """Mean squared error of `(predicted, outcome)` pairs; lower is better."""
    if not samples:
        return None
    return sum((p - y) ** 2 for p, y in samples) / len(samples)


def observe(*, on_start=None, on_end=None, on_error=None, now=time.perf_counter) -> Middleware:
    """Hooks with `duration_ms`. Hook errors are ignored."""

    def call(hook, arg):
        if hook:
            try:
                hook(arg)
            except Exception:
                pass  # observability must never change the result

    def mw(next: Decide) -> Decide:
        async def decide(model, request):
            t0 = now()
            call(on_start, {"model": model, "request": request})
            try:
                response = await next(model, request)
            except Exception as error:
                ms = (now() - t0) * 1000
                call(on_error, {"model": model, "request": request, "error": error, "duration_ms": ms})
                raise
            ms = (now() - t0) * 1000
            call(on_end, {"model": model, "request": request, "response": response, "duration_ms": ms})
            return response

        return decide

    return mw


async def fan_out(decide: Decide, request: dict, members: list, *, timeout_ms=None) -> dict:
    """Asks several models the same request at once and returns every result,
    keyed by model, in member order. Nothing is merged: combining answers is
    the app's choice. `decide`: `AsyncClient.decide` or a composed one (so
    `retry` etc. apply per member). `members`: `"provider/model"` strings or
    `{"model", "enabled"?, "questions"?}`; `questions` sends that member only
    those questions. Each result is `{"response"}`, `{"error"}`,
    `{"skipped": True}` (the member can't take the request: `Unsupported`,
    raised before any network call) or `{"timed_out": True}`, plus `ms`. Bad
    member lists raise `ValueError` before any call."""
    active = [{"model": m} if isinstance(m, str) else m for m in members]
    active = [m for m in active if m.get("enabled", True)]
    asked = request["questions"]
    if not active:
        raise ValueError("fan_out: no enabled members")
    seen = set()
    for m in active:
        if m["model"] in seen:
            raise ValueError(f"fan_out: `{m['model']}` is listed twice")
        seen.add(m["model"])
        qs = m.get("questions")
        if qs is not None and (not qs or any(q not in asked for q in qs)):
            raise ValueError(f"fan_out: `{m['model']}`: `questions` must list questions of the request")
    for q in asked:
        if not any(m.get("questions") is None or q in m["questions"] for m in active):
            raise ValueError(f"fan_out: no member is asked `{q}`")

    async def run(m):
        qs = m.get("questions")
        req = request if qs is None else {**request, "questions": {q: asked[q] for q in qs}}
        t0 = time.perf_counter()
        try:
            out = {"response": await decide(m["model"], req)}
        except Exception as e:  # noqa: BLE001 - every failure is reported, not raised
            out = {"skipped": True} if getattr(e, "code", None) == "Unsupported" else {"error": e}
        out["ms"] = round((time.perf_counter() - t0) * 1000)
        return out

    tasks = [asyncio.ensure_future(run(m)) for m in active]
    try:
        done, _ = await asyncio.wait(tasks, timeout=None if timeout_ms is None else timeout_ms / 1000)
    finally:
        for t in tasks:
            t.cancel()  # late members, or all of them if the caller cancelled
    return {
        m["model"]: t.result() if t in done else {"timed_out": True, "ms": timeout_ms}
        for m, t in zip(active, tasks)
    }

