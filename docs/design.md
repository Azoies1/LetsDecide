# Design

Why the library looks the way it does. Condensed from the original spec;
items marked *(planned)* are not built yet.

## Problem

Decision APIs are a category without a standard. Jev's System One
(`POST /v1/systemone`) is the de facto shape and Clef copies it; OpenAI and
Gemini each differ. They all share three question types: yes/no, choice, score.
A thin translation layer can hide the differences.

## Goals

- One canonical request and response on every provider.
- Switch provider by changing a string (`clef/clef-flash`).
- Negligible overhead: the network (40–250 ms) dominates translation (µs).
- Each provider is one module behind one trait; adding one never touches the core.
- Honest about differences: unsupported features error out.

## Non-goals (v1)

- Hosting, billing or a proxy service.
- Running models locally (self-hosted models are just HTTP providers).
- Emulating decisions with a chat LLM.
- Making confidence numbers comparable across providers.

## Architecture: sans-IO

```
app → binding → core.build() → HttpRequest ──► host HTTP stack ──► provider
app ← binding ← core.parse() ← HttpResponse ◄──────────────────────┘
```

The core only turns values into values. The host sends the request. This keeps
the library small, testable offline, and leaves proxies, pinning and
cancellation to the app.

## Canonical model

See `src/model.rs`.

- Questions are keyed by id; answers come back by the same id.
- Option and level ids are app-chosen and returned unchanged.
- Probabilities pass through untouched.
- Ids match `[a-z0-9_]{1,64}`.

## Provider interface

```rust
trait Provider {
    fn build(&self, model: &str, req: &DecisionRequest, cfg: &Config) -> Result<HttpRequest, Error>;
    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error>;
}
```

Two pure functions are the whole extension surface. `resolve("provider/model")`
picks the adapter.

## Provider mapping

| | Jev | Clef | OpenAI *(planned)* | Gemini *(planned)* |
|---|---|---|---|---|
| Shape | System One | System One | own | own |
| Model | body | URL | body | TBD |
| Yes/no | `noul` | `noul` | `predicate` | verify |
| Limits | 64K tokens | 64 questions | 400 questions | 1000 questions |

Jev and Clef are presets of one `SystemOne` adapter; `systemone/<model>` plus a
`base_url` covers any compatible server.

## Decisions made

- **Rust core.** One codebase for every provider mapping; bindings are generated
  later (UniFFI, napi-rs, PyO3, wasm-bindgen). Fallback if too heavy: TypeScript
  + Swift sharing the same JSON fixtures.
- **`BTreeMap` for questions.** Deterministic output; no extra crate.
- **No capability struct yet.** Limits live in the adapter. Add one when a
  second adapter needs shared validation.
- **Licence: Apache-2.0** (patent grant).

## Planned

Built only when a real need appears.

- **Adapters:** OpenAI (per-question refusals → `Refused` answer), Gemini.
- **Bindings:** generated from `spec/decision.schema.json`.
- **Middleware** (opt-in, in the host language): retry, fallback with
  capability routing, hedging, split/merge for over-limit requests, calibration
  (never silent: expose raw and calibrated), shadow, observe hooks.
- **Extras:** `confidence_source`, cost estimates from `spec/prices.json`,
  `provider_extras` escape hatch, live nightly contract tests.

## Open questions

- Gemini's exact field names (check v1beta reference before M2).
- Does OpenAI return per-option probabilities for score questions?
- Final package name: "Arbiter" is taken on GitHub; `letsdecide` for now.
- Should a hosted proxy built on this be a later product?

## Security

- Keys live in memory only; `Config`'s `Debug` redacts them.
- No telemetry; the core makes no network calls.
- Errors never include keys or request bodies.
- Browser code: a key in client code is public. Call through your backend.
