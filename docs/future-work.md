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

## Release

Not published yet. Before the first release:

- [ ] Final package name ("Arbiter" is taken; `letsdecide` for now).
- [ ] Version bump (`images` → `media` is breaking: 0.3.0) in `Cargo.toml`,
  `bindings/*/Cargo.toml`, `package.json`, `pyproject.toml`,
  `build.gradle.kts`.
- [ ] Publish: crates.io, npm (with `pkg/`), PyPI wheels (maturin), Maven
  Central, a SwiftPM release with the XCFramework attached.
- [ ] Repository secrets for the nightly live tests.
