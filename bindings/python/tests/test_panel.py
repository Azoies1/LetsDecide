import asyncio

import pytest

from letsdecide import AsyncClient, Client, LetsDecideError, mock_engine, panel_engine

REQUEST = {
    "input": "Charged twice.",
    "questions": {
        "billing": {"kind": "yesno", "instructions": "Is this about billing?"},
        "team": {"kind": "choice", "instructions": "Which team?", "options": {"pay": "P", "tech": "T"}},
    },
}


def says(p, team, source="provider"):
    probs = {"pay": 0.9 if team == "pay" else 0.1, "tech": 0.9 if team == "tech" else 0.1}
    return mock_engine(
        answers={
            "billing": {"kind": "yesno", "probability": p},
            "team": {"kind": "choice", "choice": team, "confidence": None, "probabilities": probs},
        },
        probability_source=source,
    )


class Down:
    def decide(self, model, request):
        raise LetsDecideError("ProviderUnavailable", "nope", retryable=True)


class Tiny:
    capabilities = {"max_questions": 1}

    def decide(self, model, request):
        raise AssertionError("never called: the client validates first")


class Slow:
    async def decide(self, model, request):
        await asyncio.sleep(5)


def engines(panels, **extra):
    return {
        "panel": panel_engine(panels),
        "yes": says(0.9, "pay"),
        "no": says(0.2, "tech"),
        "logp": says(0.6, "pay", "logprob"),
        "down": Down(),
        "tiny": Tiny(),
        **extra,
    }


@pytest.mark.parametrize("make", [Client, AsyncClient])
async def test_vote(make):
    c = make(engines=engines({"p": {"members": [{"model": "yes/a"}, {"model": "no/b", "weight": 2}]}}))
    res = c.decide("panel/p", REQUEST)
    res = await res if asyncio.iscoroutine(res) else res
    assert res["answers"]["team"]["choice"] == "tech"
    assert res["probability_source"] == "vote"
    assert abs(res["answers"]["billing"]["probability"] - 1 / 3) < 1e-6
    assert res["disagreed"] == ["billing", "team"]
    assert set(res["panel"]) == {"yes/a", "no/b"}


def test_disabled_skipped_and_live_config():
    members = [{"model": "tiny/a"}, {"model": "yes/b"}, {"model": "no/c", "enabled": False}]
    c = Client(engines=engines(lambda: {"p": {"members": members}}))
    res = c.decide("panel/p", REQUEST)
    assert res["panel"]["tiny/a"]["skipped"] is True
    assert "no/c" not in res["panel"]
    assert res["answers"]["team"]["choice"] == "pay"
    members[2]["enabled"] = True
    members[1]["enabled"] = False
    assert c.decide("panel/p", REQUEST)["answers"]["team"]["choice"] == "tech"


def test_below_quorum_keeps_member_error():
    c = Client(engines=engines({"p": {"members": [{"model": "yes/a"}, {"model": "down/b"}], "rules": {"quorum": 2}}}))
    with pytest.raises(LetsDecideError) as e:
        c.decide("panel/p", REQUEST)
    assert e.value.code == "ProviderUnavailable" and e.value.retryable
    assert e.value.details["reason"] == "below_quorum"
    assert "error" in e.value.panel["down/b"]


def test_mean_refuses_mixed_sources():
    c = Client(engines=engines({"p": {"members": [{"model": "yes/a"}, {"model": "logp/b"}], "rules": {"merge": "mean"}}}))
    with pytest.raises(LetsDecideError) as e:
        c.decide("panel/p", REQUEST)
    assert e.value.details["reason"] == "mixed_probability_source"


async def test_async_timeout_and_race():
    panels = {
        "t": {"members": [{"model": "yes/a"}, {"model": "slow/b"}], "rules": {"timeout_ms": 50}},
        "r": {"mode": "race", "members": [{"model": "slow/a"}, {"model": "down/b"}, {"model": "no/c"}]},
    }
    c = AsyncClient(engines=engines(panels, slow=Slow()))
    loop = asyncio.get_running_loop()
    t0 = loop.time()
    res = await c.decide("panel/t", REQUEST)
    assert loop.time() - t0 < 2
    assert res["panel"]["slow/b"]["error"]["code"] == "ProviderUnavailable"
    res = await c.decide("panel/r", REQUEST)
    assert res["answers"]["team"]["choice"] == "tech"
    assert res["probability_source"] == "provider"


def test_sync_race_and_bad_config():
    c = Client(engines=engines({
        "r": {"mode": "race", "members": [{"model": "tiny/a"}, {"model": "down/b"}]},
        "odd": {"mode": "all", "members": [{"model": "yes/a"}]},
        "empty": {"members": [{"model": "yes/a", "enabled": False}]},
    }))
    with pytest.raises(LetsDecideError) as e:
        c.decide("panel/r", REQUEST)
    assert e.value.code == "ProviderUnavailable" and e.value.panel["tiny/a"]["skipped"]
    for name, field in [("missing", "panels"), ("empty", "members"), ("odd", "mode")]:
        with pytest.raises(LetsDecideError) as e:
            c.decide(f"panel/{name}", REQUEST)
        assert e.value.code == "InvalidRequest" and e.value.details["field"] == field
