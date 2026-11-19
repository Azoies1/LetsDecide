# Future work

Things documented by a provider (or planned) but **not implemented** yet.
Each item says where it comes from and what is needed. Source pages for Gemini
were read on 2026-11-19.

## Gemini Decisions: input kinds

Sources: <https://aistudio.google.com/docs/decisions-api> (section "Input"),
<https://aistudio.google.com/api-reference/decisions/create-decision>.

The canonical model (`DecisionRequest`) only carries text/JSON `input` and
inline base64 `media`. Gemini also accepts, as typed content blocks in `input`:

- [x] **Audio** (`type: audio`): done, including `uri` and options. MIME: wav, mp3, mpeg, aiff, aac, ogg,
  flac, m4a, opus, webm, l16, alaw, mulaw.
- [x] **Video**: done, including `uri` and options. MIME: mp4, mpeg, mov, avi, webm,
  wmv, x-flv, 3gpp (the schema also lists `video/mpg`).
- [x] **Documents** (`type: document`): PDF plus DOCX, PPTX, PPT, DOC, DOTX, ODT,
  ODP, ODS, XLS, XLSX and CSV are accepted for Gemini, with `page_options` and
  `spreadsheet_options`.
  **Still unverified:** the guide page lists only `application/pdf`, the API
  schema lists the wider set. The wider set is accepted but untested live.
- [x] **Per-media options:** done for all kinds via `Media.options` (Gemini only;
  keys checked per kind, values passed through unvalidated).
- [x] **URI inputs:** done (`Media.uri`, `https://` only; no YouTube, no `gs://`). Original note: Files API URIs and public/signed HTTPS URLs (≤15 MB each).
  Not supported by Gemini: YouTube URLs, `gs://`.
- [x] **Size limits:** inline ≤100 MB per request, ≤50 MB PDF: enforced client-side.
  Still open: media-per-request limits are "same as the Interactions API" (page not
  read; see <https://aistudio.google.com/docs/interactions-overview>).
- [x] **Canonical model design:** done (`uri`, `options`, wider MIMEs). Original note: `media` covers inline bytes with the kind from
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
- [x] `usage`: cached and thought tokens now surfaced. Still open:
  `input_tokens_by_modality` and other fields.
- [x] Block reason surfaced as `DecisionResponse.block_reason`. Still open: score
  `legend` (response shape not verified).
- [x] Question/option id rules: done for Gemini (`IdRule::Gemini`). Original note: Gemini allows `[A-Za-z0-9_-]` for question keys
  and richer option keys (≤128 chars / 512 bytes); we enforce the stricter
  `[a-z0-9_]{1,64}` everywhere. Revisit if callers need the wider set.
- [ ] Open: question `noul` `criteria`
  (`yes_description` / `no_description`). Still open: `instructions` as a
  `TextContent` array.
- [x] Context limit: a lower-bound estimate (UTF-8 bytes / 4, text only) rejects
  requests that are certainly too big. Real token counts and media are not
  counted.
- [ ] Not supported by the API (no work needed, noted for callers): context
  caching, Batch API, Flex/Priority inference.

## Project roadmap (from README)

- [ ] **M3** Bindings (Swift, TypeScript, Python, Kotlin)
- [ ] **M4** Middleware (retry, fallback, split/merge)
- [ ] **M5** Evaluation (shadow mode, calibration)
- [ ] Image URLs as input; `confidence_source`; cost estimates; live nightly
  contract tests (see "Planned" in `design.md`).
