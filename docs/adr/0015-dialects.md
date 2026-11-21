# 0015: Dialects: providers as data, with named strategies

- Status: Accepted
- Date: 2026-11-20

## Context
Five decision APIs were reviewed: Jev, Clef, Gemini and OpenAI from our
verified adapters, and Perplexity Decisions from its docs as reported by
search (Microsoft-Decision-1 and Azure OpenAI Decisions were also reported).
All are variants of two wire families, System One and OpenAI. They differ in
endpoint (host, path, model or account id in the path), auth header and
scheme, input field, JSON-as-text, media encoding, response envelope, block
codes, id rules and limits. Users need to add such providers without code,
and to patch a built-in when a vendor changes its API, without a release.

## Decision
A `Dialect` is JSON that `extends` a built-in and overrides what differs:
endpoint (`base_url`, `path` with the placeholders `{model}`, `{account_id}`
and `{options.<name>}`), `auth` (header; the scheme is inferred from it),
`input_field`, `envelope`, `blocked_error_codes` and `capabilities`, plus a
**named strategy** for the part that is real logic: `media_encoding`
(`none`, `jev_assets`, `data_url_list`, `typed_blocks`, `content_parts`).
`extends` is required and fixes the wire family. Every built-in is a dialect
in `spec/dialects/*.json`; the golden fixtures are unchanged, proving the
format covers them byte for byte. Objects merge, arrays replace. Unknown
fields are errors. Dialects register like any provider: Rust `Registry`, JS
and Python `dialects`, with `replace` to override a built-in.

A first draft had thirteen fields. It was cut to the ones a real provider
needs: `family` comes from `extends`; JSON-as-text is implied by
`input_field: input` (only Gemini uses either); Jev's image header is part of
`jev_assets`; the auth scheme is inferred; `yesno_descriptions` is a
capability. A field returns only when a second provider needs it.

We rejected a general template language (JSONPath, body templates, response
mappings): question mapping needs loops and branches by kind, probabilities
are keyed by level index or by label, and errors must stay classified and
safe. That is code, and codecs already give users code with tests and the
conformance kit.

## Consequences
- New "almost compatible" providers and vendor API changes are data edits.
- Tenant dialects run no code; only `base_url` needs a host allowlist.
- A new strategy is added in the core only when a second real provider needs
  it. One-off differences are codecs.
- The `SystemOne` and `OpenAi` types are gone; `Dialect` replaces both.
