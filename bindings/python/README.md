# letsdecide (Python)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The
Rust core builds and parses; `httpx` sends. Returns plain dicts.

## Install

```sh
pip install "letsdecide[httpx]"   # or bring your own client: pip install letsdecide
```

Python 3.10+. Prebuilt wheels for Linux (glibc and musl), macOS and Windows, so
no Rust toolchain is needed.

## Usage

```python
from letsdecide import Client, LetsDecideError

# Settings per provider: a key only ever goes to its own provider.
client = Client(by_provider={"jev": {"api_key": "..."}})
# Entries also take access_token (string or callable, e.g. Clef OAuth), base_url,
# account_id, headers, options. Client also takes shared headers= and client=httpx.Client(...).
try:
    out = client.decide("jev/jev-latest", {
        "input": "Great product, arrived late.",
        "questions": {"positive": {"kind": "yesno", "instructions": "Is the review positive?"}},
    })
    print(out["answers"]["positive"])
except LetsDecideError as e:
    print(e.code, e.status, e.retryable, e.retry_after_ms, e.details)  # details: safe to log
```

Custom providers: `providers={"acme": Acme()}` (codecs with `build` and
`parse`) and `engines={"local": engine}` (`decide`); `mock_engine()` for
tests; `letsdecide.conformance.check_provider` to test them. See
[docs/custom-providers.md](https://github.com/azoies1/letsdecide/blob/HEAD/docs/custom-providers.md).

Async: `AsyncClient(...)` and `await client.decide(...)`. Change the model
string to change provider. Both clients default to a 60 s timeout; pass
`client=` to control proxies, transport and timeouts.

## More

Model strings, media input, the answer and error reference, and the other
bindings (Rust, JavaScript): <https://github.com/azoies1/letsdecide#readme>.
Apache-2.0.

## Middleware (async)

`letsdecide.middleware` wraps `AsyncClient.decide`: `retry`, `fallback`,
`hedge`, `split`, `shadow`, `calibrate`, `observe`, plus `compare`,
`fit_isotonic` and `brier_score`. Same behaviour as the JS binding; see
[MIDDLEWARE.md](https://github.com/azoies1/letsdecide/blob/HEAD/bindings/js/MIDDLEWARE.md).

```python
from letsdecide import AsyncClient
from letsdecide.middleware import compose, fallback, retry, split

keys = {"clef": {"api_key": "...", "account_id": "..."}, "jev": {"api_key": "..."}}
decide = compose(AsyncClient(by_provider=keys), split(), fallback(["jev/jev-latest"]), retry())
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
