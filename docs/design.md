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
- Ids match `[a-z0-9_]{1,64}`; Gemini allows `[A-Za-z0-9_-]{1,128}`.

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

Two wire families cover everything:

| | System One family | OpenAI |
|---|---|---|
| Providers | Jev, Clef, Gemini, custom servers | OpenAI |
| Questions | map keyed by id | ordered array, `name` = id |
| Yes/no | `type: noul` → `noul` | `type: predicate` → `probability` |
| Choice | `criteria` map; `choice` + `probabilities` map | `choices` array; `choice` + `probabilities` array |
| Score | `criteria` array; `score` + index-keyed `probabilities` | `levels` array of labels; `score` + label-keyed `probabilities` |
| Refusal | Gemini safety block → every question `Refused` | per-question `refusal` |
| Differences by preset | input is `state` (Jev, Clef) or `input` (Gemini); model also in the URL (Clef); key as Bearer or `x-goog-api-key` | |
| Limits | Clef 64, Gemini 1000 questions; Jev/Clef 10 levels, Gemini 255 options or levels | 400 questions, 10 levels |

Gemini (verified against Google's reference) turned out to be a System One preset, not its own adapter: its real
wire format matches the family (confirmed against the official
`create-decision` and `decisions-api` pages, 2026-11-19). `systemone/<model>` plus a `base_url` covers
any compatible server.

## Decisions made

Numbered records live in [adr/](adr/README.md); the list below is the original summary.

- **Rust core.** One codebase for every provider mapping; bindings are generated
  later (UniFFI, napi-rs, PyO3, wasm-bindgen). Fallback if too heavy: TypeScript
  + Swift sharing the same JSON fixtures.
- **`BTreeMap` for questions.** Deterministic output; no extra crate.
- **Gemini is a System One preset.** The spec expected a separate adapter;
  the reference showed otherwise.
- **`Refused` carries no reason.** OpenAI's refusal has none; Gemini blocks the
  whole input. Add a reason when a provider supplies one.
- **Bindings pass JSON strings.** No per-type codegen: errors come back as
  data (`{error}`) and the wrapper rethrows. Hand-written `.d.ts` for now.
- **One `media` list, kind from MIME.** Inline `data` is portable. `uri`,
  `options` and non-image kinds are Gemini-only, except public image URLs on
  OpenAI; other providers return `Unsupported`. Gemini-specific rules live in
  `src/gemini.rs`.
- **`confidence_source`, not rescaling.** When a provider omits confidence we
  read it off the probabilities and say so; numbers are never changed.
- **`provider_extras` cannot overwrite.** It adds body fields the model lacks;
  replacing what an adapter sets would make the adapter lie.
- **Prices are data.** `spec/prices.json` holds sourced prices with a date;
  the core turns tokens into an estimate. A missing price means no estimate.
- **JSON layer lives in the core** (`src/json.rs`); every binding is a thin
  wrapper that only adds a transport.
- **Middleware is `(next) => decide`, in the host language.** Retry,
  fallback, hedge, split, shadow, calibrate and observe ship for JS and
  (async) Python; `Client` stays zero-cost without them. Calibration adds
  `calibrated` next to raw values, never in place of them.
- **Swift and Kotlin through UniFFI.** One crate (`bindings/uniffi`) exports
  the same JSON pair; each language adds only a client and a transport.
- **Live tests run nightly, not per push.** They need keys and cost money;
  golden fixtures cover every push.
- **No capability struct yet.** Limits live in the adapter. Add one when a
  second adapter needs shared validation.
- **Licence: Apache-2.0** (patent grant).

## Planned

The backlog, including unverified Gemini behaviour and release steps, is in
[future-work.md](future-work.md). Built only when a real need appears.

## Open questions

- Gemini, still unverified (live calls on 2026-11-19 confirmed the answer
  shapes, CSV and URI input, and no top-level `model`): the two official pages
  disagree on safety blocks (200 + `prompt_feedback.block_reason` vs HTTP 400
  + `error.code`; both are handled); the 429 body (spend limits give
  `RESOURCE_EXHAUSTED`). Office documents are untested live.
- Final package name: "Arbiter" is taken on GitHub; `letsdecide` for now.
- Should a hosted proxy built on this be a later product?

## Security

- Keys live in memory only; `Config`'s `Debug` redacts them.
- No telemetry; the core makes no network calls.
- Errors never include keys or request bodies.
- Browser code: a key in client code is public. Call through your backend.
