# Future work

Things documented by a provider (or planned) but **not implemented** yet.
Gemini source pages were read on 2026-11-19:
[decisions-api](https://aistudio.google.com/docs/decisions-api) and
[create-decision](https://aistudio.google.com/api-reference/decisions/create-decision).

## Gemini Decisions: still unverified

Live calls (2026-11-19) confirmed: all three question kinds, CSV documents, an
`https://` image `uri`, and that replies carry no top-level `model`.

- [ ] Which safety-block shape the live API uses: `200` with
  `prompt_feedback.block_reason`, or `HTTP 400` with `error.code`. Both are
  handled; capture a real one as a fixture when it occurs.
- [ ] Body and status of RPM/TPM/RPD rate-limit errors. Docs only give
  `429 RESOURCE_EXHAUSTED` for spend limits.
- [ ] Office documents (DOCX, XLSX, ...): accepted by the schema, not yet
  sent live. CSV and PDF work.
- [ ] Media-per-request limits ("same as the Interactions API", page not read)
  and the 15 MB cap on URI media. Neither is enforced.
- [ ] The Gemini price in `spec/prices.json` came from a truncated copy of the
  pricing page; confirm it.

## Model selection

The library names no default local model; consumers pick one
([plan D4](plan-custom-and-local-providers.md#d4-default-local-model-the-consumers-choice-decided)).
Later, help the consumer choose:

- [ ] **Bake-off helper:** run the consumer's own labelled requests against
  candidate models (local and cloud) and report agreement, latency and cost
  per question, so the choice is based on their data, not ours. Builds on
  `shadow` and M5.

## Custom providers (M6)

Plan and decisions: [plan-custom-and-local-providers.md](plan-custom-and-local-providers.md).

- [x] **P1** Core: `Registry`, `Capabilities`, `DecisionResponse::check`,
  error `details`, `probability_source`, `Config.headers/options`, JSON
  entry points.
- [x] **P2** Custom codecs and engines in JS and Python; built-in `mock`
  engine; conformance kit over the golden fixture format; `ProviderError`
  wrapping of user exceptions; guide `docs/custom-providers.md`.
- [x] **P3** Dialects for the System One and OpenAI families; built-ins are
  dialects (`spec/dialects/`), in every client (Rust, JS, Python, Kotlin,
  Swift). Not yet: verified example dialects for Perplexity and
  Microsoft-Decision-1. Their docs (docs.perplexity.ai, learn.microsoft.com)
  were not reachable from the build environment on 2026-11-20.
- Dropped ([ADR 0016](adr/0016-thin-access-layer.md)): **P4** label scorer
  and `letsdecide-serve`; **P5** in-process engines, `escalate`,
  capability-aware `fallback`; **P6** Kotlin and Swift engine slots and
  reference engines. Outside the thin access layer, or already covered.
- [ ] Support tiers in the README (Built-in, Community, Custom).

## Gemini Decisions: not exposed

- Score `legend` and `input_tokens_by_modality` (response shapes not seen).
- Question `instructions` as a `TextContent` array (no gain over a string).
- A client-side check of the 262,144-token context limit. The API reports it.
- Unsupported by the API, so no work: context caching, Batch API, Flex/Priority.

## Project

- [ ] Jev pricing: not published where we could read it, so no cost estimate.
- [ ] Image URLs on Clef and Jev: their docs only describe base64.
- [ ] Python middleware for the sync `Client` (async only today).
- [ ] Android packaging for the Kotlin binding (`cargo ndk`, `jniLibs`).
- [ ] A shared `spec/decision.schema.json`, if hand-written types drift.
- [ ] Hosted proxy product: open question, not planned.

## Optional: live contract tests

Not built on purpose: every run spends real tokens, and golden fixtures
already pin each wire format. Worth adding if a provider starts changing its
format without notice. The shape it would take:

- A `live` pytest marker in `bindings/python`, deselected by default
  (`addopts = "-m 'not live'"`), run with `pytest -m live`.
- One test per provider sending all three question kinds and checking the
  canonical shape (answers for every id, probabilities summing to 1, choice
  ids round-tripping), skipped when that provider's key is not set.
- Run by hand, or on a rare schedule (weekly or manual `workflow_dispatch`)
  with keys from repository secrets. Keep requests tiny to bound cost.

## Release

First release is 0.3.0 on crates.io, npm and PyPI, from one tag (see
[releasing.md](releasing.md)). Still open: Maven and SwiftPM.

- [x] Package name: `letsdecide` on crates.io, npm and PyPI.
- [x] Version bump to 0.3.0 (`images` → `media` is breaking) in every manifest;
  the release workflow refuses a tag that doesn't match all of them.
- [x] Publish: crates.io, npm (with `pkg/`), PyPI wheels (maturin).
- [ ] Maven Central (`bindings/kotlin`).
- [ ] A SwiftPM release with the XCFramework attached (`bindings/swift`).
