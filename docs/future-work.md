# Future work

Things documented by a provider (or planned) but **not implemented** yet.
Gemini source pages were read on 2026-11-19:
[decisions-api](https://aistudio.google.com/docs/decisions-api) and
[create-decision](https://aistudio.google.com/api-reference/decisions/create-decision).

## Gemini Decisions: needs a live API key

- [ ] Which safety-block shape the live API uses: `200` with
  `prompt_feedback.block_reason`, or `HTTP 400` with `error.code`. Both are
  handled; capture real responses as fixtures.
- [ ] Whether replies carry a top-level `model` (parsed if present).
- [ ] Body and status of RPM/TPM/RPD rate-limit errors. Docs only give
  `429 RESOURCE_EXHAUSTED` for spend limits.
- [ ] Office and CSV documents: the guide lists only PDF, the API schema lists
  more. They are accepted but untested.
- [ ] Media-per-request limits ("same as the Interactions API", page not read)
  and the 15 MB cap on URI media. Neither is enforced.

## Gemini Decisions: not exposed

- Score `legend` and `input_tokens_by_modality` (response shapes not seen).
- Question `instructions` as a `TextContent` array (no gain over a string).
- A client-side check of the 262,144-token context limit. The API reports it.
- Unsupported by the API, so no work: context caching, Batch API, Flex/Priority.

## Project

- [ ] Swift and Kotlin bindings.
- [ ] Middleware: hedging, split/merge. Evaluation: shadow mode, calibration.
- [ ] Image URLs on non-Gemini providers; `confidence_source`; cost estimates;
  live nightly contract tests (see "Planned" in `design.md`).
