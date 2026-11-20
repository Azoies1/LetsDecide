import json
import logging
import pathlib
import traceback

import httpx
import pytest

from letsdecide import (
    BUILTIN,
    AsyncClient,
    Client,
    LetsDecideError,
    _core,
    classify_http,
    mock_engine,
)
from letsdecide.conformance import check_provider
from letsdecide.middleware import compose, fallback

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
REPLY = {
    "model": "acme-2",
    "results": [
        {"id": "billing", "p": 0.9},
        {"id": "team", "pick": "billing", "probs": {"billing": 0.7, "tech": 0.3}},
    ],
}


class Acme:
    """A made-up vendor."""

    capabilities = {"max_questions": 5}

    def build(self, model, request, config):
        return {
            "url": f"https://api.acme.test/v1/{model}",
            "headers": {
                "authorization": f"Bearer {config.get('api_key')}",
                "content-type": "application/json",
            },
            "body": json.dumps({"text": request["input"], "ask": list(request["questions"])}),
        }

    def parse(self, request, response):
        if response["status"] >= 300:
            raise classify_http(response)
        body = json.loads(response["body"])
        answers = {}
        for a in body["results"]:
            if request["questions"][a["id"]]["kind"] == "yesno":
                answers[a["id"]] = {"kind": "yesno", "probability": a["p"]}
            else:
                answers[a["id"]] = {
                    "kind": "choice",
                    "choice": a["pick"],
                    "confidence": None,
                    "probabilities": a["probs"],
                }
        return {"model": body["model"], "answers": answers, "input_tokens": None}


def http_client(handler):
    return httpx.Client(transport=httpx.MockTransport(handler))


def responder(status, body, headers=None):
    return lambda req: httpx.Response(status, json=body, headers=headers or {})


def test_builtin_matches_core():
    assert sorted(BUILTIN) == sorted(json.loads(_core.builtins())["ok"])


def test_custom_codec_gets_config_headers_and_checks():
    sent = {}

    def handler(req):
        sent.update(url=str(req.url), headers=dict(req.headers))
        return httpx.Response(200, json=REPLY)

    c = Client(
        client=http_client(handler), api_key="k", headers={"x-gw": "g"}, providers={"acme": Acme()}
    )
    out = c.decide("acme/acme-2", REQUEST)
    assert sent["url"] == "https://api.acme.test/v1/acme-2"
    assert sent["headers"]["authorization"] == "Bearer k"
    assert sent["headers"]["x-gw"] == "g"
    assert out["answers"]["billing"] == {"kind": "yesno", "probability": 0.9}
    assert out["probability_source"] == "provider"


def test_capabilities_checked_before_codec():
    def handler(req):
        raise AssertionError("must not send")

    big = {"input": "x", "questions": {f"q{i}": {"kind": "yesno", "instructions": "?"} for i in range(6)}}
    c = Client(client=http_client(handler), providers={"acme": Acme()})
    with pytest.raises(LetsDecideError) as e:
        c.decide("acme/m", big)
    assert e.value.code == "Unsupported"
    assert e.value.details == {
        "reason": "too_many_questions",
        "provider": "acme",
        "limit": 5,
        "actual": 6,
    }


def test_malformed_custom_answers_are_bad_response():
    bad = {**REPLY, "results": [{"id": "billing", "p": 1.7}, REPLY["results"][1]]}
    c = Client(client=http_client(responder(200, bad)), providers={"acme": Acme()})
    with pytest.raises(LetsDecideError) as e:
        c.decide("acme/m", REQUEST)
    assert e.value.code == "BadResponse"
    assert e.value.details["reason"] == "invalid_number"
    assert e.value.details["provider"] == "acme"


def test_http_errors_classified_like_builtins():
    c = Client(
        client=http_client(responder(429, {}, {"retry-after": "3"})), providers={"acme": Acme()}
    )
    with pytest.raises(LetsDecideError) as e:
        c.decide("acme/m", REQUEST)
    assert (e.value.code, e.value.retryable, e.value.retry_after_ms) == ("RateLimited", True, 3000)


def test_user_exceptions_become_provider_error_without_message(caplog):
    class Leaky(Acme):
        def parse(self, request, response):
            raise ValueError("body was: secret-token-123")

    c = Client(client=http_client(responder(200, REPLY)), providers={"acme": Leaky()})
    with pytest.raises(LetsDecideError) as e:
        c.decide("acme/m", REQUEST)
    err = e.value
    assert err.code == "ProviderError"
    assert err.provider == "acme"
    assert "secret" not in str(err)
    assert "secret" in str(err.original)  # kept for the user's own debugging
    text = "".join(traceback.format_exception(err))
    assert "secret" not in text  # but not printed with the traceback
    with caplog.at_level(logging.ERROR):
        logging.getLogger("t").exception("failed", exc_info=err)
    assert "secret" not in caplog.text


def test_names_refused_unless_replaced():
    with pytest.raises(LetsDecideError) as e:
        Client(providers={"openai": Acme()})
    assert e.value.details["reason"] == "name_taken"
    with pytest.raises(LetsDecideError):
        Client(providers={"acme": Acme()}, engines={"acme": mock_engine()})
    with pytest.raises(LetsDecideError) as e:
        Client(providers={"Bad/Name": Acme()})
    assert e.value.code == "InvalidRequest"
    with pytest.raises(TypeError):
        Client(providers={"acme": object()})
    Client(providers={"openai": Acme()}, replace=["openai"])


def test_mock_engine_needs_no_network_or_httpx_client():
    c = Client(engines={"mock": mock_engine({"billing": {"kind": "yesno", "probability": 0.8}})})
    out = c.decide("mock/any", REQUEST)
    assert out["answers"]["billing"] == {"kind": "yesno", "probability": 0.8}
    assert out["answers"]["team"]["choice"] == "billing"
    assert out["answers"]["team"]["probabilities"] == {"billing": 0.5, "tech": 0.5}


def test_sync_client_rejects_async_engine():
    class AsyncEngine:
        async def decide(self, model, request):
            return {}

    with pytest.raises(TypeError):
        Client(engines={"a": AsyncEngine()}).decide("a/m", REQUEST)


async def test_async_client_engine_falls_back_to_codec():
    class Down:
        async def decide(self, model, request):
            raise LetsDecideError("ProviderUnavailable", "down", retryable=True)

    transport = httpx.MockTransport(responder(200, REPLY))
    c = AsyncClient(
        client=httpx.AsyncClient(transport=transport),
        providers={"acme": Acme()},
        engines={"local": Down()},
    )
    res = await compose(c, fallback(["acme/acme-2"]))("local/tiny", REQUEST)
    assert res["model"] == "acme-2"


GOOD = {
    "name": "acme yes/no and choice",
    "model": "acme/acme-2",
    "config": {"api_key": "k"},
    "request": REQUEST,
    "http": {
        "url": "https://api.acme.test/v1/acme-2",
        "headers": [["authorization", "Bearer k"], ["content-type", "application/json"]],
        "body": {"text": "Charged twice.", "ask": ["billing", "team"]},
    },
    "response": {"status": 200, "body": REPLY},
    "expected": {
        "ok": {
            "model": "acme-2",
            "input_tokens": None,
            "answers": {
                "billing": {"kind": "yesno", "probability": 0.9},
                "team": {
                    "kind": "choice",
                    "choice": "billing",
                    "confidence": None,
                    "probabilities": {"billing": 0.7, "tech": 0.3},
                },
            },
        }
    },
}


def test_conformance_kit_reports_problems():
    limited = {
        "name": "429",
        "model": "acme/m",
        "request": REQUEST,
        "response": {"status": 429, "headers": [["retry-after", "2"]], "body": {}},
        "expected": {"error": {"code": "RateLimited", "retry_after_ms": 2000}},
    }
    wrong = {**GOOD, "name": "wrong url", "http": {**GOOD["http"], "url": "https://elsewhere"}}
    report = check_provider(Acme(), [GOOD, limited, wrong])
    assert report["passed"] == 2
    assert report["results"][2]["problems"] == [
        "url: got https://api.acme.test/v1/acme-2, want https://elsewhere"
    ]


def test_conformance_kit_replays_core_golden_fixtures():
    class Shim:
        """Delegates to the core: proves the fixture format round-trips."""

        def build(self, model, request, cfg):
            out = json.loads(_core.build(f"openai/{model}", json.dumps(request), json.dumps(cfg)))
            if "error" in out:
                raise LetsDecideError(**out["error"])
            return out["ok"]

        def parse(self, request, response):
            out = json.loads(_core.parse("openai/x", json.dumps(request), json.dumps(response)))
            if "error" in out:
                raise LetsDecideError(**out["error"])
            return out["ok"]

    root = pathlib.Path(__file__).resolve().parents[3] / "tests" / "fixtures" / "openai"
    fixtures = [{"name": p.name, **json.loads(p.read_text())} for p in sorted(root.glob("*.json"))]
    report = check_provider(Shim(), fixtures)
    assert report["failed"] == 0, report["results"]
