# Future work

Things documented by a provider (or planned) but **not implemented** yet.
Each item says where it comes from and what is needed. Source pages for Gemini
were read on 2026-11-19.

## Gemini Decisions: input kinds

Sources: <https://aistudio.google.com/docs/decisions-api> (section "Input"),
<https://aistudio.google.com/api-reference/decisions/create-decision>.

The canonical model (`DecisionRequest`) only carries text/JSON `input` and
inline base64 `media`. Gemini also accepts, as typed content blocks in `input`:

- [x] **Audio** (`type: audio`, inline `data` only): done. Not yet: `uri`; optional
  `channels`, `sample_rate`, `processing`. MIME: wav, mp3, mpeg, aiff, aac, ogg,
  flac, m4a, opus, webm, l16, alaw, mulaw.
- [x] **Video** (inline `data` only): done. Not yet: `uri`; optional
  `resolution`, `fps`, `start_offset`/`end_offset` (strings like `"10.5s"`),
  `processing` (`static` | `agentic`), `name`. MIME: mp4, mpeg, mov, avi, webm,
  wmv, x-flv, 3gpp (the schema also lists `video/mpg`).
- [x] **PDF** (`type: document`): done. Not yet, per the API schema: DOCX, PPTX,
  PPT, DOC, DOTX, ODT, ODP, ODS, XLS, XLSX, CSV. Options: `page_options`
  (`start_page`, `end_page`, `content_layers`), `spreadsheet_options`
  (`visible_cells`, `sheet_index`, `content_layers`).
  **Open conflict:** the guide page lists only `application/pdf`, the API
  schema lists the wider set. Test against the live API before accepting more.
- [ ] **Image options:** `resolution` (`low|medium|high|ultra_high`).
- [ ] **URI inputs:** Files API URIs and public/signed HTTPS URLs (≤15 MB each).
  Not supported by Gemini: YouTube URLs, `gs://`.
- [ ] **Size limits to enforce:** inline base64 ≤100 MB per request (≤50 MB for
  PDF); media-per-request limits are "same as the Interactions API" (page not
  read; see <https://aistudio.google.com/docs/interactions-overview>).
- [ ] **Canonical model design:** `media` covers inline bytes with the kind from
  the MIME (other providers return `Unsupported`). URLs, per-media options and
  the wider document MIMEs need a richer shape; add it when first needed.

## Gemini Decisions: still unverified

- [ ] Which safety-block shape the live API uses: `200` with
  `prompt_feedback.block_reason` (create-decision page) or `HTTP 400` with
  `error.code` (decisions-api page). Both are handled; capture real responses
  as fixtures once an API key is available.
- [ ] Whether replies carry a top-level `model` (parsed if present).
- [ ] Exact body and status for rate limits. Docs: spend limits give `429
  RESOURCE_EXHAUSTED`; RPM/TPM/RPD errors are not described.
- [ ] `usage` has more fields than we read (`input_tokens_by_modality`,
  `total_cached_tokens`, `total_thought_tokens`, ...); only
  `total_input_tokens` is surfaced.
- [ ] Gemini-specific answer fields not surfaced: score `legend`, and
  `prompt_feedback.block_reason_message` (`Refused` carries no reason).
- [ ] Question/option id rules: Gemini allows `[A-Za-z0-9_-]` for question keys
  and richer option keys (≤128 chars / 512 bytes); we enforce the stricter
  `[a-z0-9_]{1,64}` everywhere. Revisit if callers need the wider set.
- [ ] Question `instructions` as a `TextContent` array, and `noul` `criteria`
  (`true_description` / `false_description`): supported by Gemini, not exposed.
- [ ] Context limit 262,144 tokens across input and questions is not checked
  client-side.
- [ ] Not supported by the API (no work needed, noted for callers): context
  caching, Batch API, Flex/Priority inference.

## Model selection

The library names no default local model; consumers pick one
([plan D4](plan-custom-and-local-providers.md#d4-default-local-model-the-consumers-choice-decided)).
Later, let a decision do the choosing:

- [ ] **Decision-routed model choice:** a small, cheap decision question
  ("is this request simple?", "does it contain an image?") picks which model
  answers the real request, e.g. a local model for easy cases, a cloud model
  for hard ones. Built as middleware over the registry.
- [ ] **Bake-off helper:** run the consumer's own labelled requests against
  candidate models (local and cloud) and report agreement, latency and cost
  per question, so the choice is based on their data, not ours. Builds on
  `shadow` and M5.

## Project roadmap (from README)

- [ ] **M3** Bindings (Swift, TypeScript, Python, Kotlin)
- [ ] **M4** Middleware (retry, fallback, split/merge)
- [ ] **M5** Evaluation (shadow mode, calibration)
- [ ] Image URLs as input; `confidence_source`; cost estimates; live nightly
  contract tests (see "Planned" in `design.md`).
