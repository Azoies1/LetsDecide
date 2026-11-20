import json

import httpx
import pytest

from letsdecide import Client, LetsDecideError, dialect, resolve_dialect

REQUEST = {"input": "x", "questions": {"q": {"kind": "yesno", "instructions": "?"}}}


def recorder(status, body):
    sent = []

    def handler(req):
        sent.append({"url": str(req.url), "headers": dict(req.headers), "body": json.loads(req.content)})
        return httpx.Response(status, json=body)

    return sent, httpx.Client(transport=httpx.MockTransport(handler))


def test_a_dialect_is_a_provider_described_as_data():
    sent, http = recorder(200, {"answers": {"q": {"noul": 0.75}}})
    c = Client(
        client=http,
        by_provider={"acme": {"api_key": "k"}},
        headers={"x-trace": "t"},
        dialects={"acme": {"extends": "jev", "base_url": "https://api.acme.test", "path": "/v1/decisions"}},
    )
    out = c.decide("acme/acme-1", REQUEST)
    assert sent[0]["url"] == "https://api.acme.test/v1/decisions"
    assert sent[0]["headers"]["authorization"] == "Bearer k"
    assert sent[0]["headers"]["x-trace"] == "t"
    assert sent[0]["body"]["questions"] == {"q": {"type": "noul", "instructions": "?"}}
    assert out["answers"]["q"] == {"kind": "yesno", "probability": 0.75}


def test_patching_a_builtin_after_an_api_change_needs_only_data():
    sent, http = recorder(200, {"answers": {"q": {"noul": 0.5}}})
    c = Client(
        client=http,
        by_provider={"gemini": {"api_key": "k"}},
        dialects={"gemini": {"extends": "gemini", "path": "/v1/decisions"}},
        replace=["gemini"],
    )
    c.decide("gemini/gemini-decision-2", REQUEST)
    assert sent[0]["url"] == "https://generativelanguage.googleapis.com/v1/decisions"
    assert sent[0]["headers"]["x-goog-api-key"] == "k"


def test_dialects_get_the_same_checks():
    _, http = recorder(200, {"answers": {"q": {"noul": 1.5}}})
    c = Client(
        client=http,
        dialects={"tiny": {"extends": "systemone", "base_url": "https://h.test", "capabilities": {"max_questions": 1}}},
    )
    with pytest.raises(LetsDecideError) as e:
        c.decide("tiny/m", REQUEST)
    assert e.value.code == "BadResponse"
    assert e.value.details["provider"] == "tiny"
    two = {"input": "x", "questions": {"a": REQUEST["questions"]["q"], "b": REQUEST["questions"]["q"]}}
    with pytest.raises(LetsDecideError) as e:
        c.decide("tiny/m", two)
    assert e.value.details["reason"] == "too_many_questions"
    assert c.capabilities("tiny/m")["max_questions"] == 1


def test_a_bad_dialect_fails_when_the_client_is_made():
    with pytest.raises(LetsDecideError) as e:
        resolve_dialect({"extends": "jev", "pathh": "/x"})
    assert "unknown field `pathh`" in str(e.value)
    with pytest.raises(LetsDecideError) as e:
        Client(dialects={"bad": {"extends": "nope"}})
    assert e.value.details["reason"] == "invalid_config"
    assert e.value.details["provider"] == "bad"
    with pytest.raises(LetsDecideError) as e:
        Client(dialects={"jev": {"extends": "jev"}})
    assert e.value.details["reason"] == "name_taken"


def test_builtin_dialects_can_be_read_and_extended():
    assert dialect("gemini")["auth"]["header"] == "x-goog-api-key"
    full = resolve_dialect({"extends": "openai", "base_url": "https://az.test/openai"})
    assert full["path"] == "/v1/decisions"
