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

client = Client(api_key="...")  # also: access_token= (OAuth string or callable, e.g. for Clef), base_url=, account_id=, headers=, options=, client=httpx.Client(...)
try:
    out = client.decide("jev/jev-latest", {
        "input": "Great product, arrived late.",
        "questions": {"positive": {"kind": "yesno", "instructions": "Is the review positive?"}},
    })
    print(out["answers"]["positive"])
except LetsDecideError as e:
    print(e.code, e.status, e.retryable, e.retry_after_ms, e.details)  # details: safe to log
```

Several providers? `by_provider={"jev": {"api_key": ...}, "gemini": {...}}` gives each its own settings.
Custom providers: `providers={"acme": Acme()}` (codecs with `build` and
`parse`) and `engines={"local": engine}` (`decide`); `mock_engine()` for
tests; `letsdecide.conformance.check_provider` to test them. See
[docs/custom-providers.md](../../docs/custom-providers.md).

Async: `AsyncClient(...)` and `await client.decide(...)`. Change the model
string to change provider. Pass `client=` to control proxies, transport and
timeouts.

## Middleware (async)

`letsdecide.middleware` wraps `AsyncClient.decide`: `retry`, `fallback`,
`hedge`, `split`, `shadow`, `calibrate`, `observe`, plus `compare`,
`fit_isotonic` and `brier_score`. Same behaviour as the JS binding; see
[MIDDLEWARE.md](../js/MIDDLEWARE.md).

```python
from letsdecide import AsyncClient
from letsdecide.middleware import compose, fallback, retry, split

decide = compose(AsyncClient(api_key="..."), split(), fallback(["jev/jev-latest"]), retry())
out = await decide("clef/clef-flash", request)
out["attempts"], out.get("fell_back_from"), out.get("parts")
```

## Build and test

```sh
python -m venv .venv && . .venv/bin/activate
pip install maturin pytest pytest-asyncio httpx
maturin develop
pytest
```
