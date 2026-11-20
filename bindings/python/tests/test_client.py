import json

import httpx
import pytest

from letsdecide import AsyncClient, Client, LetsDecideError

REQUEST = {
    "input": "Charged twice.",
    "questions": {
        "billing": {"kind": "yesno", "instructions": "Is this about billing?"},
        "team": {
            "kind": "choice",
            "instructions": "Which team?",
            "options": {"billing": "Payments", "tech": "Bugs"},
        },
    },
}


def client(handler, **config):
    return Client(client=httpx.Client(transport=httpx.MockTransport(handler)), **config)


def test_decide_builds_sends_and_parses():
    sent = {}

    def handler(req):
        sent.update(url=str(req.url), key=req.headers["x-goog-api-key"], body=json.loads(req.content))
        return httpx.Response(
            200,
            json={
                "answers": {
                    "billing": {"type": "noul", "noul": 0.9},
                    "team": {
                        "type": "choice",
                        "choice": "billing",
                        "confidence": 0.8,
                        "probabilities": {"billing": 0.8, "tech": 0.2},
                    },
                },
                "usage": {"total_input_tokens": 7},
            },
        )

    out = client(handler, api_key="k").decide("gemini/gemini-decision-1", REQUEST)
    assert sent["url"] == "https://generativelanguage.googleapis.com/v1beta/decisions"
    assert sent["key"] == "k"
    assert sent["body"]["model"] == "gemini-decision-1"
    assert out["answers"]["billing"] == {"kind": "yesno", "probability": 0.9}
    assert out["answers"]["team"]["choice"] == "billing"
    assert out["input_tokens"] == 7


def test_switching_provider_is_one_string():
    urls = []

    def handler(req):
        urls.append(str(req.url))
        return httpx.Response(401, json={})

    c = client(handler, api_key="k", account_id="a")
    for m in ["jev/jev-latest", "openai/gpt-6-luna", "clef/clef-flash"]:
        with pytest.raises(LetsDecideError) as e:
            c.decide(m, REQUEST)
        assert e.value.code == "Auth"
    assert urls == [
        "https://api.typesafe.ai/v1/systemone",
        "https://api.openai.com/v1/decisions",
        "https://api.cloudflare.com/client/v4/accounts/a/ai/run/@cf/cloudflare/clef-flash",
    ]


def test_errors_are_typed_and_carry_retry_info():
    c = client(lambda r: httpx.Response(429, json={}, headers={"retry-after": "2"}), api_key="k")
    with pytest.raises(LetsDecideError) as e:
        c.decide("jev/jev-latest", REQUEST)
    assert e.value.code == "RateLimited"
    assert e.value.retryable is True
    assert e.value.retry_after_ms == 2000
    assert e.value.status == 429


def test_invalid_request_makes_no_network_call():
    def handler(req):
        pytest.fail("must not send")

    bad = {"input": "x", "questions": {"Bad-Id": {"kind": "yesno", "instructions": "?"}}}
    with pytest.raises(LetsDecideError) as e:
        client(handler).decide("jev/jev-latest", bad)
    assert e.value.code == "InvalidRequest"


async def test_async_client():
    transport = httpx.MockTransport(lambda r: httpx.Response(401, json={}))
    c = AsyncClient(client=httpx.AsyncClient(transport=transport), api_key="k")
    with pytest.raises(LetsDecideError) as e:
        await c.decide("jev/jev-latest", REQUEST)
    assert e.value.code == "Auth"


def test_clef_oauth_token_callable_is_refreshed_per_request():
    seen = []

    def handler(req):
        seen.append(req.headers["authorization"])
        return httpx.Response(401, json={})

    n = iter(range(1, 10))
    c = client(handler, account_id="a", access_token=lambda: f"tok{next(n)}")
    for _ in range(2):
        with pytest.raises(LetsDecideError) as e:
            c.decide("clef/clef-flash", REQUEST)
        assert e.value.code == "Auth"
    assert seen == ["Bearer tok1", "Bearer tok2"]


def test_gemini_and_clef_fail_fast_without_credentials():
    def handler(req):
        raise AssertionError("must not send")

    c = client(handler, account_id="a")
    for model in ("gemini/gemini-decision-1", "clef/clef-flash"):
        with pytest.raises(LetsDecideError) as e:
            c.decide(model, REQUEST)
        assert e.value.code == "InvalidRequest"


def test_config_headers_are_sent_and_replace_same_named_ones():
    sent = {}

    def handler(req):
        sent.update(req.headers)
        return httpx.Response(401, json={})

    c = client(handler, api_key="k", headers={"x-gateway": "g", "Authorization": "Token t"})
    with pytest.raises(LetsDecideError):
        c.decide("jev/jev-latest", REQUEST)
    assert sent["x-gateway"] == "g"
    assert sent["authorization"] == "Token t"


def test_errors_carry_safe_details():
    def handler(req):
        pytest.fail("must not send")

    many = {
        "input": "x",
        "questions": {f"q{i}": {"kind": "yesno", "instructions": "secret"} for i in range(65)},
    }
    with pytest.raises(LetsDecideError) as e:
        client(handler).decide("clef/clef-flash", many)
    assert e.value.code == "Unsupported"
    assert e.value.details == {
        "reason": "too_many_questions",
        "provider": "clef",
        "limit": 64,
        "actual": 65,
    }
    assert "secret" not in str(e.value)
