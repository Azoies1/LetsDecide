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
> [Verification status](#verification-status). Image input works with every
> provider except System One; audio, video and PDF with Gemini. See the
> [roadmap](#roadmap).

## Install

```sh
cargo add letsdecide              # Rust
npm install letsdecide            # JavaScript / TypeScript
pip install "letsdecide[httpx]"   # Python
```

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
| `clef/<model>` | Cloudflare Clef | `api_key`, `account_id` |
| `gemini/gemini-decision-1` | Google Gemini Decisions | `api_key` |
| `openai/gpt-6-luna` | OpenAI Decisions | `api_key` |
| `systemone/<model>` | Any System One compatible server | `base_url`, optional `api_key` |

`input` is a string or any JSON value. Providers that only take text get JSON
serialised to a string.

### Media

Add `media: [{ "mime_type": "image/png", "data": "<base64>" }]` to a request.
Inline base64 only: it is the one form every provider accepts. The kind
(image, audio, video, document) comes from the MIME type.

| Kind | `gemini` | `openai` | `clef` | `jev` | `systemone` |
|---|---|---|---|---|---|
| image | typed `image` blocks | `input_image` data URLs | `images` data URLs, ≤4, PNG/JPEG/WebP | `assets` + placeholders in `state`, PNG/JPEG | no |
| audio, video, PDF | typed blocks | `Unsupported` | `Unsupported` | `Unsupported` | `Unsupported` |

Jev image requests send the `TypeSafe-Danger-Zone-Images` header and need a
vision model such as `jev-preview`.

### Answers

| Question | Answer | Notes |
|---|---|---|
| `yesno` | `YesNo { probability }` | P(yes) |
| `choice` | `Choice { choice, confidence, probabilities }` | keyed by your option ids |
| `score` | `Score { value, confidence, probabilities }` | `probabilities` keyed by your level ids |
| any | `Refused` | Provider declined (OpenAI refusal, Gemini safety block, as `prompt_feedback.block_reason` or HTTP 400 `error.code`). Other answers still succeed |

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
| `Unsupported` | Provider can't take this request (Clef >64 questions, Gemini >1000, OpenAI >400, Jev/Clef/OpenAI >10 score levels, Gemini >255 options or levels, unsupported Gemini image type) | No |
| `Auth` | 401 / 403 | No |
| `RateLimited` | 429; `retry_after_ms` set when the provider sends `Retry-After` | Yes |
| `ProviderUnavailable` | 5xx | Yes |
| `BadResponse` | 2xx but the body doesn't parse. Likely an API change | No |

## JavaScript / TypeScript

The same core compiles to WebAssembly (146 KB gzipped) for Node, browsers and
edge runtimes. The wrapper sends requests with `fetch`.

```js
import { Client } from "letsdecide";

const client = new Client({ apiKey: process.env.GEMINI_API_KEY });

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
import { Client } from "letsdecide";
import { compose, retry, fallback } from "letsdecide/middleware.js";

const decide = compose(client, fallback(["jev/jev-latest"]), retry());
const res = await decide("clef/clef-flash", request);
res.attempts; res.fellBackFrom; // see when a fallback happened
```

Retry (backoff, `Retry-After`) and fallback (also routes around `Unsupported`)
are plain `(next) => decide` wrappers. See [bindings/js/MIDDLEWARE.md](bindings/js/MIDDLEWARE.md).

## Python

```python
from letsdecide import Client  # pip install "letsdecide[httpx]"

res = Client(api_key="...").decide("jev/jev-latest", request)
```

Sync `Client` and `AsyncClient`, both over `httpx`. See
[bindings/python/README.md](bindings/python/README.md).

## Verification status

| Provider | Source of the wire format |
|---|---|
| OpenAI | OpenAI's API reference |
| Jev | Jev's official OpenAPI spec (`api.typesafe.ai/openapi.json`) |
| Clef | Cloudflare's official Clef docs |
| Gemini | Google's official pages, read 2026-11-19: [create-decision](https://aistudio.google.com/api-reference/decisions/create-decision) and [decisions-api](https://aistudio.google.com/docs/decisions-api). Endpoint, `x-goog-api-key`, request and response shapes, limits and image MIME types match. Unverified: a top-level `model` in replies, which safety-block shape applies when (both handled), and the exact 429 body. Audio, video and PDF input are implemented (inline only); URIs and other document types are not |

Fixtures in `tests/fixtures/` encode these shapes. Corrections welcome.

## Roadmap

Built in small steps; each lands only when needed. Details in
[docs/design.md](docs/design.md).

- [x] **M1** Canonical model, sans-IO core, System One adapter, golden fixtures
- [x] **M2** OpenAI and Gemini adapters, refusals
- [x] Verify Gemini against Google's official reference
- [x] Audio, video and PDF input (Gemini, inline). URLs and per-media options: [docs/future-work.md](docs/future-work.md)
- [x] **M3** Bindings: TypeScript/WASM, Python. Swift and Kotlin when asked for
- [x] **M4** Retry and fallback (JS). Next: hedging, split/merge, shadow, calibration, observe
- [ ] **M5** Evaluation (shadow mode, calibration)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a provider is one file plus fixtures.

## Licence

[Apache-2.0](LICENSE)
