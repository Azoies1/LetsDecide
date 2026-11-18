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

> **Status: early (v0.2).** Adapters for Jev, Clef, Gemini, OpenAI and any
> System One compatible server. Wire formats come from public docs; see
> [Verification status](#verification-status). Image/audio/PDF input is not
> supported yet. See the [roadmap](#roadmap).

## Usage

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
| `clef/<model>` | Cloudflare Clef | `api_key`, `account_id` |
| `gemini/gemini-decision-1` | Google Gemini Decisions | `api_key` |
| `openai/gpt-6-luna` | OpenAI Decisions | `api_key` |
| `systemone/<model>` | Any System One compatible server | `base_url`, optional `api_key` |

`input` is a string or any JSON value. Providers that only take text get JSON
serialised to a string.

### Answers

| Question | Answer | Notes |
|---|---|---|
| `yesno` | `YesNo { probability }` | P(yes) |
| `choice` | `Choice { choice, confidence, probabilities }` | keyed by your option ids |
| `score` | `Score { value, confidence, probabilities }` | `probabilities` keyed by your level ids |
| any | `Refused` | Provider declined (OpenAI refusal, Gemini safety block). Other answers still succeed |

Numbers are the provider's, never rescaled. `confidence` is `None` when the
provider sends none. Jev/Clef/Gemini scores are the probability-weighted
**0-based level index** (e.g. `0.75` between level 0 and 1); OpenAI's `score`
is passed through as sent.

### Errors

Every failure is an `Error` with a stable `code` and a `retryable()` flag.
Keys and request bodies are never included.

| Code | When | Retryable |
|---|---|---|
| `InvalidRequest` | Fails validation (empty questions, bad or duplicate ids, <2 options) or the provider returned 4xx | No |
| `Unsupported` | Provider can't take this request (Clef >64 questions, Gemini >1000, OpenAI >400, Jev/Clef/OpenAI >10 score levels) | No |
| `Auth` | 401 / 403 | No |
| `RateLimited` | 429; `retry_after_ms` set when the provider sends `Retry-After` | Yes |
| `ProviderUnavailable` | 5xx | Yes |
| `BadResponse` | 2xx but the body doesn't parse. Likely an API change | No |

## Verification status

| Provider | Source of the wire format |
|---|---|
| OpenAI | OpenAI's API reference |
| Jev | Jev's official OpenAPI spec (`api.typesafe.ai/openapi.json`) |
| Clef | Cloudflare's official Clef docs |
| Gemini | **A third-party mirror** of the reference (EvoLink). Host and `x-goog-api-key` header per Google's docs. Official page not yet read |

Fixtures in `tests/fixtures/` encode these shapes. Corrections welcome.

## Roadmap

Built in small steps; each lands only when needed. Details in
[docs/design.md](docs/design.md).

- [x] **M1** Canonical model, sans-IO core, System One adapter, golden fixtures
- [x] **M2** OpenAI and Gemini adapters, refusals
- [ ] Verify Gemini against Google's official reference; image, audio and PDF input
- [ ] **M3** Bindings (Swift, TypeScript, Python, Kotlin)
- [ ] **M4** Middleware (retry, fallback, split/merge)
- [ ] **M5** Evaluation (shadow mode, calibration)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a provider is one file plus fixtures.

## Licence

[Apache-2.0](LICENSE)
