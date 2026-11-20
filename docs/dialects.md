# Dialects

A dialect describes a provider as data. Every built-in is one
(`spec/dialects/*.json`), and you can add your own for any API that speaks one
of the two known wire families with small differences: another host or path,
auth header, input field, response envelope, media encoding or limits.

Use a dialect when:

- a vendor copied System One (Jev's shape) or OpenAI's Decisions shape;
- you run a compatible server behind your own path or auth;
- **a built-in provider changes its API** and you want the fix today, without
  waiting for a release: extend the built-in and override what changed.

Write a [codec](custom-providers.md) instead when the API maps questions or
answers differently. Dialects are deliberately not a template language
(see [ADR 0015](adr/0015-dialects.md)).

## Quick start

```js
const client = new Client(
  { byProvider: { acme: { apiKey: process.env.ACME_KEY } } },
  { dialects: { acme: { extends: "jev", base_url: "https://api.acme.ai", path: "/v1/decisions" } } },
);
await client.decide("acme/acme-decide-1", request);
```

```python
client = Client(
    by_provider={"acme": {"api_key": os.environ["ACME_KEY"]}},
    dialects={"acme": {"extends": "jev", "base_url": "https://api.acme.ai", "path": "/v1/decisions"}},
)
```

```rust
let acme = Dialect::from_json(r#"{ "extends": "jev", "base_url": "https://api.acme.ai", "path": "/v1/decisions" }"#)?;
let mut providers = Registry::builtin();
providers.register("acme", acme)?;
```

A dialect gets everything a built-in gets: request validation against its
`capabilities`, the response check, classified errors with `details` naming
the provider, per-provider settings, and every middleware.

## Patching a built-in after an API change

Say Gemini moves its endpoint to `/v1/decisions`:

```js
new Client(cfg, { dialects: { gemini: { extends: "gemini", path: "/v1/decisions" } }, replace: ["gemini"] });
```

`replace` is required because `gemini` is a built-in name (ADR 0008). The rest
of Gemini's dialect (auth, media, limits, block codes) is kept. When the
library ships the fix, drop the override.

## Reading the built-ins

```js
await dialect("gemini");   // the built-in as JSON
await resolveDialect({ extends: "openai", base_url: "https://..." }); // full result, validated
```

Python: `dialect("gemini")`, `resolve_dialect({...})`. Rust:
`Dialect::builtin_json("gemini")`, `Dialect::from_json(...)`. The sources are
`spec/dialects/*.json`.

## Format

| Field | Values | Default | Meaning |
|---|---|---|---|
| `extends` | `jev`, `clef`, `gemini`, `openai`, `systemone` | none | Start from this built-in. Objects merge key by key; arrays and values replace |
| `family` | `systemone`, `openai` | required | Question and answer shape: `questions` map with `noul`/`choice`/`score`, or ordered array with `predicate`/`choice`/`score` |
| `base_url` | `https://...` or `http://...` | none | Default endpoint. A provider's `baseUrl` setting overrides it. None means the caller must set one |
| `path` | `/...` | required | Placeholders: `{model}`, `{account_id}`. Values must be one path segment (no `/`, `?`, `#`, `%`, `..`) |
| `auth.header` | header name | `authorization` | Where the credential goes |
| `auth.scheme` | `bearer`, `raw` | `bearer` | `Bearer <access_token or api_key>`, or the `api_key` as-is |
| `auth.required` | bool | `false` | Fail before sending when no credential is set |
| `input_field` | `state`, `input` | `state` | System One only: where the input goes |
| `json_input` | `value`, `text` | `value` | JSON input as a JSON value, or serialised to a string |
| `media_encoding` | see below | `none` | How media is attached |
| `media_headers` | `[[name, value], ...]` | `[]` | Sent only when the request has media (Jev's image gate) |
| `envelope` | key | none | Replies wrapped in an object under this key (Clef: `result`) |
| `blocked_error_codes` | `[code, ...]` | `[]` | HTTP 400 `error.code` values that mean "input blocked": every answer becomes `refused` with `block_reason` |
| `yesno_descriptions` | bool | `false` | Yes/no questions may carry `yes_description` / `no_description` |
| `capabilities` | object | no limits, no media | `max_questions`, `max_options`, `max_levels`, `max_media`, `media` (MIME types, `type/*`), `question_kinds`, `wide_ids`, `probabilities`, `locality` |

Keys starting with `_` are comments (`_doc`). Unknown keys are an error, so a
typo such as `pathh` fails with `unknown field \`pathh\`` when the dialect is
registered (Python, Rust) or first used (JS, whose core loads asynchronously;
call `resolveDialect` to check up front).

### Media encodings

Each is tested code in the core; a dialect picks one by name.

| Name | Used by | What it sends |
|---|---|---|
| `none` | custom System One servers | no media |
| `jev_assets` | Jev | an `assets` map plus placeholders appended to the input; `format` from the MIME subtype |
| `data_url_list` | Clef | an `images` array of `data:` URLs |
| `typed_blocks` | Gemini | the input as typed blocks (`text`, `image`, `audio`, `video`, `document`), inline or by URI, with per-media options |
| `content_parts` | OpenAI | `input_image` content parts (data URLs or public image URLs) |

`content_parts` is OpenAI-family only; the others are System One only.

## Examples

Shapes reported for other vendors. **These come from secondary sources, not
verified against official docs.** Check before relying on them.

```jsonc
// A System One copy at another path, e.g. Perplexity Decisions (reported: POST /v1/decisions, Bearer)
{ "extends": "jev", "base_url": "https://api.perplexity.ai", "path": "/v1/decisions",
  "media_encoding": "none", "media_headers": [], "capabilities": { "media": [] } }

// System One behind a different key header, e.g. Microsoft-Decision-1 on Foundry
// (reported: {endpoint}/providers/microsoft/v1/systemone, `api-key` header)
{ "extends": "systemone", "base_url": "https://YOUR-RESOURCE.services.ai.azure.com/providers/microsoft",
  "auth": { "header": "api-key", "scheme": "raw", "required": true } }

// An OpenAI-compatible endpoint, e.g. Azure OpenAI Decisions
{ "extends": "openai", "base_url": "https://YOUR-RESOURCE.openai.azure.com/openai",
  "auth": { "header": "api-key", "scheme": "raw", "required": true } }
```

## Security

A dialect is data: registering one runs no code. Two things still matter when
dialects come from users or tenants:

- **URLs.** A tenant-chosen `base_url` makes your server call that host. Allow
  only hosts you trust before registering a tenant's dialect.
- **Credentials** go only to the provider's own `byProvider` entry (ADR 0014),
  so a tenant dialect can't receive another provider's key.

## When a dialect isn't enough

If an API needs something these fields can't say (a different question or
answer mapping, a new media scheme, several calls), write a codec. If a
second real provider needs the same thing, it becomes a new named strategy in
the core.
