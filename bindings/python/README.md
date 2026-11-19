# letsdecide (Python)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The
Rust core builds and parses; `httpx` sends. Returns plain dicts.

## Install

```sh
pip install "letsdecide[httpx]"   # or bring your own client: pip install letsdecide
```

## Usage

```python
from letsdecide import Client, LetsDecideError

client = Client(api_key="...")  # also: base_url=, account_id=, headers=, options=, client=httpx.Client(...)
try:
    out = client.decide("jev/jev-latest", {
        "input": "Great product, arrived late.",
        "questions": {"positive": {"kind": "yesno", "instructions": "Is the review positive?"}},
    })
    print(out["answers"]["positive"])
except LetsDecideError as e:
    print(e.code, e.status, e.retryable, e.retry_after_ms, e.details)  # details: safe to log
```

Async: `AsyncClient(...)` and `await client.decide(...)`. Change the model
string to change provider. Pass `client=` to control proxies, transport and
timeouts.

## Build and test

```sh
python -m venv .venv && . .venv/bin/activate
pip install maturin pytest pytest-asyncio httpx
maturin develop
pytest
```
