import asyncio

import pytest

from letsdecide import AsyncClient, LetsDecideError, mock_engine
from letsdecide.middleware import compose, fan_out, retry

REQUEST = {
    "input": "Charged twice.",
    "questions": {
        "billing": {"kind": "yesno", "instructions": "Is this about billing?"},
        "team": {"kind": "choice", "instructions": "Which team?", "options": {"pay": "P", "tech": "T"}},
    },
}


def says(p):
    return mock_engine(answers={"billing": {"kind": "yesno", "probability": p}})


class Down:
    def decide(self, model, request):
        raise LetsDecideError("ProviderUnavailable", "x", retryable=True)


class Tiny:
    capabilities = {"max_questions": 1}

    def decide(self, model, request):
        raise AssertionError("never called: the client validates first")


class Slow:
    async def decide(self, model, request):
        await asyncio.sleep(5)


class Spy:
    def __init__(self):
        self.seen = None

    def decide(self, model, request):
        self.seen = sorted(request["questions"])
        return says(0.5).decide(model, request)


def client(**extra):
    return AsyncClient(engines={"yes": says(0.9), "no": says(0.1), "down": Down(), "tiny": Tiny(), "slow": Slow(), **extra})


async def test_every_raw_result_in_member_order():
    out = await fan_out(client().decide, REQUEST, ["no/a", "yes/b", "down/c", "tiny/d"])
    assert list(out) == ["no/a", "yes/b", "down/c", "tiny/d"]
    assert out["yes/b"]["response"]["answers"]["billing"]["probability"] == 0.9
    assert out["down/c"]["error"].code == "ProviderUnavailable"
    assert out["tiny/d"]["skipped"] is True
    assert all(isinstance(r["ms"], int) for r in out.values())


async def test_disabled_and_routing():
    a, b = Spy(), Spy()
    out = await fan_out(
        client(a=a, b=b).decide,
        REQUEST,
        [{"model": "a/x", "questions": ["billing"]}, {"model": "b/x", "questions": ["team"]}, {"model": "yes/c", "enabled": False}],
    )
    assert (a.seen, b.seen) == (["billing"], ["team"])
    assert list(out) == ["a/x", "b/x"]


async def test_timeout_returns_what_arrived():
    loop = asyncio.get_running_loop()
    t0 = loop.time()
    out = await fan_out(client().decide, REQUEST, ["yes/a", "slow/b"], timeout_ms=50)
    assert loop.time() - t0 < 2
    assert "response" in out["yes/a"]
    assert out["slow/b"]["timed_out"] is True


async def test_composed_middleware_per_member():
    calls = []

    class Flaky:
        def decide(self, model, request):
            calls.append(1)
            if len(calls) == 1:
                raise LetsDecideError("RateLimited", "x", retryable=True)
            return says(0.7).decide(model, request)

    async def no_sleep(_):
        pass

    decide = compose(client(flaky=Flaky()), retry(sleep=no_sleep))
    out = await fan_out(decide, REQUEST, ["flaky/a"])
    assert "response" in out["flaky/a"] and len(calls) == 2


@pytest.mark.parametrize(
    "members",
    [
        [],
        [{"model": "yes/a", "enabled": False}],
        ["yes/a", "yes/a"],
        [{"model": "yes/a", "questions": ["nope"]}],
        [{"model": "yes/a", "questions": []}],
        [{"model": "yes/a", "questions": ["billing"]}],
    ],
)
async def test_bad_member_lists_raise_before_any_call(members):
    with pytest.raises(ValueError):
        await fan_out(client().decide, REQUEST, members)
