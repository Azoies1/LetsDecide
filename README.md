# letsdecide

Write a decision request once. Switch providers with one string.

Decision APIs (Jev, Clef, OpenAI, Gemini, self-hosted) answer the same three
question types (yes/no, choice, score) but disagree on wire format.
`letsdecide` hides that behind one canonical request and response.

- **No gateway.** You call providers directly with your own keys.
- **Sans-IO.** The core builds an HTTP request and parses the reply. It never
  opens a socket, so you keep control of your HTTP stack, proxies and cancellation.
- **Tiny.** Two dependencies (`serde`, `serde_json`). No async runtime.
- **Honest.** Provider limits surface as errors, not silent degradation.

> **Status: early (v0.2), not yet published** to crates.io, npm, PyPI, Maven
> or SwiftPM. See [docs/future-work.md](docs/future-work.md#release).
> Adapters for Jev, Clef, Gemini, OpenAI and any
> System One compatible server. Bindings for Rust, TypeScript/WASM, Python,
> Kotlin and Swift. Wire formats come from public docs; see
> [Verification status](#verification-status). Image input works on all but
> `systemone`; audio, video and documents on Gemini only. See the [roadmap](#roadmap).

## Usage (Rust)

```rust,no_run
use letsdecide::*;

# fn send(_: HttpRequest) -> HttpResponse { unimplemented!() }
# fn main() -> Result<(), Error> {
let req: DecisionRequest = serde_json::from_value(serde_json::json!({
    "input": "Great product, arrived late.",
    "questions": {
        "positive": { "kind": "yesno", "instructions": "Is the review positive?" },
        "topic": {
            "kind": "choice",
            "instructions": "Main topic?",
            "options": { "shipping": "Delivery", "quality": "Product" }
        }
    }
})).unwrap();

let cfg = Config { api_key: Some("...".into()), ..Default::default() };

// "provider/model": change this string to change provider.
let (provider, model) = resolve("jev/jev-latest")?;

let http = provider.build(model, &req, &cfg)?; // pure: no I/O
let res = send(http);                           // your HTTP client
let decision = provider.parse(&req, &res)?;     // pure: no I/O

// Answers come back keyed by your question ids.
println!("{:?}", decision.answers["positive"]);
# Ok(()) }
```

### Model strings

| String | Target | Config needed |
|---|---|---|
| `jev/<model>` | Jev System One API | `api_key` |
| `clef/<model>` | Cloudflare Clef | `account_id` plus `access_token` (OAuth) or `api_key` |
| `gemini/gemini-decision-1` | Google Gemini Decisions | `api_key` (required) |
| `openai/gpt-6-luna` | OpenAI Decisions | `api_key` |
| `systemone/<model>` | Any System One compatible server | `base_url`, optional `api_key` |
| `<yours>/<model>` | Your own provider, see [Custom providers](#custom-providers) | yours |

The last column is that provider's entry in the client's `byProvider` map
(Python `by_provider`; Rust: the `Config` you pass to `build`). Credentials and
endpoints are only ever set per provider, so a key can't reach another
provider. Each entry may also set `headers` and `options` (for custom
providers). Client-wide `headers` (tracing, gateways) go to every provider and
must not hold secrets.

`input` is a string or any JSON value. Providers that only take text get JSON
serialised to a string.

### Authentication

- **Gemini:** a static API key (`api_key`), sent as `x-goog-api-key`. Create one in
  Google AI Studio. A missing key fails with `InvalidRequest` before anything is sent.
- **Clef:** an OAuth access token (`access_token`, sent as `Authorization: Bearer`)
  or a plain API token (`api_key`). The token wins if both are set. Clef also needs
  your Cloudflare `account_id`.
- **Tokens expire, the core does not refresh them** (it does no I/O). Run your OAuth
  flow in your app, then set `accessToken: async () => freshToken()` in that
  provider's entry (Python `"access_token": callable`), or in Rust set
  `Config.access_token` before each `build`. The function is called on every
  request, so refresh logic lives in one place.
- **Keys are per provider.** A top-level `apiKey` is refused (`InvalidRequest`,
  `invalid_config`): with fallback, hedging or panels one client talks to several
  providers, and a shared key would be sent to all of them.
- Keys and tokens are redacted from `Debug` output and never appear in errors.

```js
// One client, two providers, each with its own credentials
const client = new Client({
  byProvider: {
    clef: { accountId: ACCOUNT_ID, accessToken: () => getToken() },
    gemini: { apiKey: process.env.GEMINI_API_KEY },
  },
});
```

### Media

Add `media: [{ "mime_type": "image/png", "data": "<base64>" }]` to a request.
Use `data` (base64, every provider) or `uri` (Gemini; public image URLs on
OpenAI). The kind (image,
audio, video, document) comes from the MIME type. Gemini also takes per-media
`options`, passed through as-is.

| Kind | `gemini` | `openai` | `clef` | `jev` | `systemone` |
|---|---|---|---|---|---|
| image | typed `image` blocks | `input_image` data or http(s) URLs | `images` data URLs, ≤4, PNG/JPEG/WebP | `assets` + placeholders in `state`, PNG/JPEG | no |
| audio, video, document | typed blocks | `Unsupported` | `Unsupported` | `Unsupported` | `Unsupported` |

Jev image requests send the `TypeSafe-Danger-Zone-Images` header and need a
vision model such as `jev-preview`.

### Answers

| Question | Answer | Notes |
|---|---|---|
| `yesno` | `YesNo { probability }` | P(yes) |
| `choice` | `Choice { choice, confidence, confidence_source, probabilities }` | keyed by your option ids |
| `score` | `Score { value, confidence, confidence_source, probabilities }` | `probabilities` keyed by your level ids |
| any | `Refused` | Provider declined (OpenAI refusal, Gemini safety block, as `prompt_feedback.block_reason` or HTTP 400 `error.code`). Other answers still succeed |

Every response has `probability_source` (`provider` for every built-in; later
`logprob` or `nli` for local models) and is checked before you get it: every
question answered, kinds and ids match, probabilities in `[0, 1]`.

Numbers are the provider's, never rescaled. `confidence_source` says where
`confidence` came from: `provider`, or `probabilities` when the provider sent
none and it was read off the chosen option (or most likely level). Each
provider computes confidence its own way, so don't compare it across
providers. Jev/Clef/Gemini scores are the probability-weighted
**0-based level index** (e.g. `0.75` between level 0 and 1); OpenAI's `score`
is passed through as sent.

### Provider extras and cost

`provider_extras: { ... }` adds top-level fields to the provider's body as-is,
for options not modelled yet. A key the adapter already sets is rejected, never
overwritten.

The bindings add `estimated_cost_usd` to a response when the model's price is
in [`spec/prices.json`](spec/prices.json) (OpenAI, Clef, Gemini; Jev publishes
none). In Rust: `estimate_cost_usd(model, input_tokens)`. It is an estimate of direct
provider billing: regional premiums, long-context multipliers and cache
discounts are not applied, and a `base_url` proxy or gateway may bill differently.

### Errors

Every failure is an `Error` with a stable `code` and a `retryable()` flag.
Most also carry `details`: a stable `reason` (`too_many_questions`,
`media_not_accepted`, `unknown_provider`, `missing_answer`, …) plus the
provider, question id, limit and actual count, MIME type or config field name.
Errors never contain keys, header values, input, media, instructions,
descriptions, bodies or config values; tests check this.

| Code | When | Retryable |
|---|---|---|
| `InvalidRequest` | Fails validation (empty questions, bad or duplicate ids, <2 options, a `provider_extras` key the adapter sets) or the provider returned 4xx | No |
| `Unsupported` | Provider can't take this request (provider limits such as Clef >64 questions or >10 score levels, or a Gemini-only feature on another provider: non-image media, `uri` other than OpenAI image URLs, `options`, yes/no descriptions) | No |
| `Auth` | 401 / 403 | No |
| `RateLimited` | 429; `retry_after_ms` set when the provider sends `Retry-After` | Yes |
| `ProviderUnavailable` | 5xx | Yes |
| `BadResponse` | 2xx but the body doesn't parse or fails the response check. Likely an API change, or a bug in a custom provider | No |
| `ProviderError` | Reserved for errors thrown by custom provider code | No |

## Custom providers

Not supported yet? Plug it in, in JS, Python or Rust. Your provider gets the
same request validation, response checks, error codes and middleware as the
built-ins. Full guide, engines, the mock engine and the conformance kit:
[docs/custom-providers.md](docs/custom-providers.md).

```js
import { Client, classifyHttp, mockEngine } from "letsdecide";

const acme = {
  capabilities: { max_questions: 50 },
  build: (model, req, cfg) => ({ url: "https://api.acme.ai/v1/decide", headers: { authorization: `Bearer ${cfg.apiKey}` }, body: JSON.stringify(toAcme(model, req)) }),
  parse: async (req, res) => { if (res.status >= 300) throw await classifyHttp(res); return fromAcme(JSON.parse(res.body), req); },
};
const client = new Client({ byProvider: { acme: { apiKey } } }, { providers: { acme }, engines: { mock: mockEngine() } });
await client.decide("acme/acme-decide-2", request); // or "mock/any" in tests
```

Python: `Client(by_provider={"acme": {"api_key": ...}}, providers={"acme": Acme()}, engines={"mock": mock_engine()})`.
Rust:

```rust,no_run
use letsdecide::*;

struct Acme;
impl Provider for Acme {
    fn capabilities(&self, _model: &str) -> Capabilities {
        Capabilities { max_questions: Some(50), ..Default::default() }
    }
    fn build(&self, model: &str, req: &DecisionRequest, cfg: &Config) -> Result<HttpRequest, Error> {
        # let _ = (model, req, cfg); unimplemented!()
        // Map `req` to Acme's wire format; `apply_headers(&mut http, cfg)` last.
    }
    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        if res.status >= 300 { return Err(http_error(res)); } // same classification as built-ins
        # let _ = req; unimplemented!()
        // Map Acme's reply to answers keyed by your question ids.
    }
}

# fn main() -> Result<(), Error> {
# let (req, res): (DecisionRequest, HttpResponse) = unimplemented!();
let mut providers = Registry::builtin();
providers.register("acme", Acme)?;            // error if the name is taken
let http = providers.build("acme/decide-2", &req, &Config::default())?; // capabilities checked first
let out = providers.parse("acme/decide-2", &req, &res)?;                // response checked after
# Ok(()) }
```

A server that already speaks System One needs no code: use
`systemone/<model>` with `base_url` (and `headers` if it wants other auth).

## JavaScript / TypeScript

The same core compiles to WebAssembly (146 KB gzipped) for Node, browsers and
edge runtimes. The wrapper sends requests with `fetch`.

```js
import { Client } from "letsdecide";

const client = new Client({ byProvider: { gemini: { apiKey: process.env.GEMINI_API_KEY } } });

const result = await client.decide("gemini/gemini-decision-1", {
  input: "I was charged twice.",
  questions: {
    billing: { kind: "yesno", instructions: "Is this about billing?" },
  },
});

result.answers.billing; // { kind: "yesno", probability: 0.97 }
```

Failures throw `LetsDecideError` with `code`, `status`, `retryable` and
`retryAfterMs`. In Node, pass the WASM bytes: `new Client(cfg, { wasm })`.
Build and test: `cd bindings/js && npm run build && npm test` (needs the
`wasm32-unknown-unknown` target and `wasm-bindgen-cli`).

### Middleware (opt-in)

```js
import { Client, compose, retry, fallback, split } from "letsdecide";

const decide = compose(client, split(), fallback(["jev/jev-latest"]), retry());
const res = await decide("clef/clef-flash", request);
res.attempts; res.fellBackFrom; // see when a fallback happened
```

Plain `(next) => decide` wrappers: `retry` (backoff, `Retry-After`),
`fallback` (also routes around `Unsupported`), `hedge` (latency hedging),
`split` (over-limit requests in parts), `shadow` + `compare` (mirror traffic to
another model and measure agreement), `calibrate` + `fitIsotonic` (calibrated
values next to raw ones) and `observe` (timing hooks). Python has the same set
for `AsyncClient`. See [bindings/js/MIDDLEWARE.md](bindings/js/MIDDLEWARE.md).

## Python

```python
import os
from letsdecide import Client  # not published yet; build from bindings/python

client = Client(by_provider={"jev": {"api_key": os.environ["JEV_API_KEY"]}})
res = client.decide("jev/jev-latest", request)
```

Sync `Client` and `AsyncClient`, both over `httpx`; middleware in
`letsdecide.middleware`. See [bindings/python/README.md](bindings/python/README.md).

## Kotlin and Swift

Both use the same core through UniFFI (`bindings/uniffi`), with thin clients
over `java.net.http` and `URLSession`. See
[bindings/kotlin/README.md](bindings/kotlin/README.md) and
[bindings/swift/README.md](bindings/swift/README.md).

## Verification status

| Provider | Source of the wire format |
|---|---|
| OpenAI | OpenAI's API reference |
| Jev | Jev's official OpenAPI spec (`api.typesafe.ai/openapi.json`) |
| Clef | Cloudflare's official Clef docs |
| Gemini | Google's [create-decision](https://aistudio.google.com/api-reference/decisions/create-decision) and [decisions-api](https://aistudio.google.com/docs/decisions-api) pages, read 2026-11-19, and live calls (all kinds, CSV, URI image). Open points: [docs/future-work.md](docs/future-work.md) |

Fixtures in `tests/fixtures/` encode these shapes. Corrections welcome. CI
makes no real provider calls; live contract tests are an optional addition
(see [docs/future-work.md](docs/future-work.md#optional-live-contract-tests)).

## Roadmap

Built in small steps; each lands only when needed. Details in
[docs/design.md](docs/design.md).

- [x] **M1** Canonical model, sans-IO core, System One adapter, golden fixtures
- [x] **M2** OpenAI and Gemini adapters, refusals
- [x] Verify Gemini against Google's official reference
- [x] Audio, video and document input, URIs and per-media options (Gemini)
- [x] **M3** Bindings: TypeScript/WASM, Python, Kotlin, Swift (UniFFI)
- [x] **M4** Middleware: retry, fallback, hedging, split/merge, observe (JS and Python)
- [x] **M5** Evaluation: shadow mode with agreement, calibration (isotonic fit, Brier score)
- [x] Extras: OpenAI image URLs, `confidence_source`, cost estimates, `provider_extras`
- [ ] Optional: live contract tests (not built: each run spends real tokens)
- [ ] First release (crates.io, npm, PyPI, Maven, SwiftPM)
- [ ] **M6** Custom providers and local models ([plan](docs/plan-custom-and-local-providers.md)). Done: registry, capabilities, response checks, error details, `headers`/`options` (P1); custom codecs and engines in JS and Python, mock engine, conformance kit (P2). Next: dialects (P3), local label scorer and sidecar (P4)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a provider is one file plus fixtures.

## Licence

[Apache-2.0](LICENSE)
