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
- Users can add providers we don't ship (another cloud, an in-house service)
  without forking: register them under a name; they get the same validation,
  response checks, errors and middleware as the built-ins.
- Honest about differences: unsupported features error out.

## Non-goals (v1)

- Hosting, billing or a proxy service.
- **Running models.** No scorer, sidecar or bundled engines
  ([ADR 0016](adr/0016-thin-access-layer.md)). A local server that speaks a
  known wire format is reached like any provider.
- Getting decisions by **parsing a model's generated text**.
- Making confidence numbers comparable across providers.
- Picking a model for the user (local or cloud). See future-work.md.

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
    fn capabilities(&self, model: &str) -> Capabilities;
    fn build(&self, model: &str, req: &DecisionRequest, cfg: &Config) -> Result<HttpRequest, Error>;
    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error>;
}
```

Three pure functions are the whole extension surface. A `Registry` maps
prefixes to providers (built-ins plus the user's); `resolve("provider/model")`
is the shortcut for built-ins only. Going through a registry (as every
binding does):

```
Capabilities::check(req)  →  build  →  host sends  →  parse  →  DecisionResponse::check(req)
```

so every provider, built-in or custom, gets the same guarantees: requests it
can't take fail before any I/O (`Unsupported`, with details), and malformed
answers fail as `BadResponse` instead of reaching the app.

### Extension ladder

| Rung | User writes | Status |
|---|---|---|
| 0. Config | `base_url`, `headers`, `options` | built |
| 1. Dialect | data describing a System One / OpenAI variant ([dialects.md](dialects.md)) | built; every built-in is one |
| 2. Codec | `build` + `parse` (+ `capabilities`) in their language | built: Rust (`Registry`), JS, Python |
| 3. Engine | one async `decide()`; for non-HTTP providers and mocks | built: JS, Python |

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
- **Providers are dialects** (ADR 0015). Every built-in is data in
  `spec/dialects/*.json`; the two wire families are code. Media encodings
  are named strategies, not templates.
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
- **Several models: fan out, never merge.** `fanOut` returns every model's raw
  answer. Voting, averaging and quorum are app policy and stay out of the core
  (see [concurrent-models.md](concurrent-models.md)).
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
- **No live tests in CI.** Real calls need keys and spend tokens on every
  run; golden fixtures cover the wire formats. Live contract tests are an
  optional add-on (see future-work).
- **`Capabilities` is data, checked by the core.** Limits, accepted media
  (`type/*` patterns), question kinds, `probabilities` source and
  `locality` (cloud, network, device). Replaced per-adapter limit code once
  custom providers and routing needed the same checks.
- **Registry is a value, not a global.** Each client owns one. Custom names are
  free (`[a-z0-9_-]{1,64}`) and **registering a taken name is an error**
  (`name_taken`) unless the caller uses `replace`. We rejected a reserved
  namespace (`x-acme/`): its only benefit is that a future built-in can never
  clash, and that clash already fails loudly at start-up. Readable model
  strings matter more. The changelog lists every new built-in prefix.
- **Every response is checked** (`DecisionResponse::check`): every question
  answered once, kinds match, ids are the caller's, probabilities finite and
  in `[0, 1]` (1e-3 slack). Score `value` is only checked to be finite.
- **Errors carry `details`**: a stable `reason` (snake_case) plus provider,
  question/option ids, limit and actual counts, MIME type, config field
  *name*. Only fields that are safe to log exist, so leaking content is not
  possible by construction. Tests plant a canary in keys, headers, input,
  media, instructions, descriptions, bodies and config values and check no
  error contains it.
- **serde messages are not echoed** when they could quote a value (`invalid
  type: string "…"`); only position and category are kept. Messages naming
  our own fields (`missing field`) are kept.
- **`ProviderError`** is reserved for errors thrown by user code (custom
  codecs and engines), so they can't be confused with ours.
- **`probability_source`** on every response (`provider`, `logprob`, `nli`):
  numbers from different sources are never on one scale.
- **No default model.** Consumers choose (ADR 0012).
- **Credentials per provider only** (ADR 0014). Clients have no top-level
  key: each provider's key, token, endpoint and options live in its own
  `byProvider` entry, so routing middleware can't send a key to the wrong
  provider. Client-wide headers are for non-secrets.
- **Thin access layer** (ADR 0016): the library reaches decision APIs; it
  does not run models.
- **Licence: Apache-2.0** (patent grant).

## Planned

The backlog, including unverified Gemini behaviour and release steps, is in
[future-work.md](future-work.md). Built only when a real need appears.

Custom providers: P1–P3 in
[plan-custom-and-local-providers.md](plan-custom-and-local-providers.md) (closed; P4–P6 dropped).

## Open questions

- Gemini, still unverified (live calls on 2026-11-19 confirmed the answer
  shapes, CSV and URI input, and no top-level `model`): the two official pages
  disagree on safety blocks (200 + `prompt_feedback.block_reason` vs HTTP 400
  + `error.code`; both are handled); the 429 body (spend limits give
  `RESOURCE_EXHAUSTED`). Office documents are untested live.
- Final package name: "Arbiter" is taken on GitHub; `letsdecide` for now.
- Should a hosted proxy built on this be a later product?

## Security

- Keys live in memory only; `Config`'s `Debug` redacts the key, header
  values, option values and the `base_url` query string.
- No telemetry; the core makes no network calls.
- Errors never include keys, header values, input, media, instructions,
  descriptions, bodies or config values (enforced by canary tests). Errors
  thrown by user code are the user's responsibility; bindings will wrap them
  as `ProviderError` (planned, P2).
- `headers` reject names that aren't HTTP tokens and values with line breaks
  (no header injection).
- Multi-tenant apps that load dialects from tenant settings must allowlist
  hosts: a tenant-chosen URL is an SSRF vector. Values filled into a dialect
  path (`{model}`, `{account_id}`) must be a single path segment.
- Browser code: a key in client code is public. Call through your backend.
