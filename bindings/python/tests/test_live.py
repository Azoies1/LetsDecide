"""Live contract tests: real calls, checked against the canonical shape.

Run with `pytest -m live`. Each provider runs only when its keys are set:
GEMINI_API_KEY; OPENAI_API_KEY; CLEF_API_KEY (or CLEF_ACCESS_TOKEN) and
CLEF_ACCOUNT_ID; JEV_API_KEY. The nightly workflow sets them from secrets, so
a provider changing its wire format fails CI instead of users.
"""

import base64
import math
import os

import pytest

from letsdecide import Client

pytestmark = pytest.mark.live

REQUEST = {
    "input": "Order #12: I was charged twice for the same item. Please fix it today.",
    "questions": {
        "billing": {"kind": "yesno", "instructions": "Is this about billing?"},
        "team": {
            "kind": "choice",
            "instructions": "Which team should handle it?",
            "options": {"payments": "Payments and refunds", "tech": "Bugs and outages"},
        },
        "urgency": {
            "kind": "score",
            "instructions": "How urgent is it?",
            "levels": [
                {"id": "low", "description": "Can wait"},
                {"id": "mid", "description": "Within a day"},
                {"id": "high", "description": "Right now"},
            ],
        },
    },
}

PROVIDERS = {
    "gemini/gemini-decision-1": lambda e: e.get("GEMINI_API_KEY") and {"api_key": e["GEMINI_API_KEY"]},
    "openai/gpt-6-luna": lambda e: e.get("OPENAI_API_KEY") and {"api_key": e["OPENAI_API_KEY"]},
    "clef/clef-flash": lambda e: (e.get("CLEF_API_KEY") or e.get("CLEF_ACCESS_TOKEN"))
    and e.get("CLEF_ACCOUNT_ID")
    and {
        "api_key": e.get("CLEF_API_KEY"),
        "access_token": e.get("CLEF_ACCESS_TOKEN"),
        "account_id": e["CLEF_ACCOUNT_ID"],
    },
    "jev/jev-latest": lambda e: e.get("JEV_API_KEY") and {"api_key": e["JEV_API_KEY"]},
}


def client_for(model):
    config = PROVIDERS[model](os.environ)
    if not config:
        pytest.skip(f"no credentials for {model}")
    return Client(**config)


def check_probabilities(probs, ids):
    assert set(probs) <= set(ids)
    assert all(0 <= p <= 1 for p in probs.values())
    assert math.isclose(sum(probs.values()), 1, abs_tol=0.02)


def check_shape(request, out):
    assert set(out["answers"]) == set(request["questions"])
    for qid, q in request["questions"].items():
        a = out["answers"][qid]
        assert a["kind"] == q["kind"], qid
        if a["kind"] == "yesno":
            assert 0 <= a["probability"] <= 1
        elif a["kind"] == "choice":
            assert a["choice"] in q["options"]
            check_probabilities(a["probabilities"], q["options"])
        else:
            ids = [lvl["id"] for lvl in q["levels"]]
            assert 0 <= a["value"] <= len(ids) - 1
            check_probabilities(a["probabilities"], ids)
    assert out["input_tokens"] is None or out["input_tokens"] > 0


@pytest.mark.parametrize("model", list(PROVIDERS))
def test_all_question_kinds(model):
    out = client_for(model).decide(model, REQUEST)
    check_shape(REQUEST, out)
    # A clear-cut input: a wrong answer here means a mapping bug, not a model quirk.
    assert out["answers"]["billing"]["probability"] > 0.5
    assert out["answers"]["team"]["choice"] == "payments"


def test_gemini_csv_document():
    model = "gemini/gemini-decision-1"
    csv = base64.b64encode(b"name,amount\nalice,10\nbob,-5\n").decode()
    req = {
        "input": "A ledger.",
        "media": [{"mime_type": "text/csv", "data": csv}],
        "questions": {"negative": {"kind": "yesno", "instructions": "Is any amount negative?"}},
    }
    out = client_for(model).decide(model, req)
    check_shape(req, out)
    assert out["answers"]["negative"]["probability"] > 0.5


def test_gemini_uri_image():
    model = "gemini/gemini-decision-1"
    req = {
        "input": "An image.",
        "media": [
            {
                "mime_type": "image/png",
                "uri": "https://www.google.com/images/branding/googlelogo/2x/googlelogo_color_272x92dp.png",
            }
        ],
        "questions": {"logo": {"kind": "yesno", "instructions": "Is this a company logo?"}},
    }
    out = client_for(model).decide(model, req)
    check_shape(req, out)
