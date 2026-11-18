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

> **Status: early (v0.1).** Only the System One adapter exists (Jev, Clef,
> compatible servers). Its wire format is assumed from the design spec and
> needs verifying against provider docs. See the [roadmap](#roadmap).

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
| `systemone/<model>` | Any System One compatible server | `base_url`, optional `api_key` |

### Errors

Every failure is an `Error` with a stable `code` and a `retryable()` flag.
Keys and request bodies are never included.

| Code | When | Retryable |
|---|---|---|
| `InvalidRequest` | Fails validation (empty questions, bad or duplicate ids, <2 options) or the provider returned 4xx | No |
| `Unsupported` | Provider can't take this request (e.g. Clef: >64 questions) | No |
| `Auth` | 401 / 403 | No |
| `RateLimited` | 429; `retry_after_ms` set when the provider sends `Retry-After` | Yes |
| `ProviderUnavailable` | 5xx | Yes |
| `BadResponse` | 2xx but the body doesn't parse. Likely an API change | No |

## Roadmap

Built in small steps; each lands only when needed. Details in
[docs/design.md](docs/design.md).

- [x] **M1** Canonical model, sans-IO core, System One adapter, golden fixtures
- [ ] **M2** OpenAI and Gemini adapters, refusals
- [ ] **M3** Bindings (Swift, TypeScript, Python, Kotlin)
- [ ] **M4** Middleware (retry, fallback, split/merge)
- [ ] **M5** Evaluation (shadow mode, calibration)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a provider is one file plus fixtures.

## Licence

[Apache-2.0](LICENSE)
