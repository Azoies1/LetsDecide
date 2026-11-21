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

```kotlin
Client(
    byProvider = mapOf("acme" to ProviderSettings(apiKey = System.getenv("ACME_KEY"))),
    dialects = mapOf("acme" to JSONObject("""{"extends": "jev", "base_url": "https://api.acme.ai", "path": "/v1/decisions"}""")),
)
```

```swift
try Client(
    byProvider: ["acme": ProviderSettings(apiKey: key)],
    dialects: ["acme": ["extends": "jev", "base_url": "https://api.acme.ai", "path": "/v1/decisions"]])
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

Python: `dialect("gemini")`, `resolve_dialect({...})`. Kotlin and Swift:
`dialect("gemini")`, `resolveDialect(...)`. Rust:
`Dialect::builtin_json("gemini")`, `Dialect::from_json(...)`. The sources are
`spec/dialects/*.json`.

## Format

Every field except `extends` is optional: it defaults to the value in the
built-in you extend. Most dialects set two or three fields.

| Field | Values | Meaning |
|---|---|---|
| `extends` | `jev`, `clef`, `gemini`, `openai`, `systemone` | **Required.** The built-in to start from; it also fixes the wire family (System One for all but `openai`). Objects merge key by key; arrays and values replace. Use `systemone` or `openai` for a fresh one |
| `base_url` | `https://...` or `http://...` | Default endpoint. A provider's `baseUrl` setting overrides it |
| `path` | `/...` | Placeholders: `{model}`, `{account_id}`, `{options.<name>}` (from that provider's `options`). Values must be one path segment (no `/`, `?`, `#`, `%`, `..`) |
| `auth.header` | header name | Where the credential goes. The scheme is inferred: `authorization` gets `Bearer <token or key>`, any other header (`x-api-key`, `api-key`) gets the raw `api_key` |
| `auth.scheme` | `bearer`, `raw` | Only to override the inferred scheme |
| `auth.required` | bool | Fail before sending when no credential is set. Inherited: `false` for `jev`, `openai` and `systemone`, `true` for `clef` and `gemini` |
| `input_field` | `state`, `input` | System One only. `state`: JSON input as-is (Jev, Clef). `input`: JSON input as text (Gemini) |
| `media_encoding` | see below | How media is attached |
| `envelope` | key | Replies wrapped in an object under this key (Clef: `result`) |
| `blocked_error_codes` | `[code, ...]` | HTTP 400 `error.code` values that mean "input blocked": every answer becomes `refused` with `block_reason` |
| `capabilities` | object | `max_questions`, `max_options`, `max_levels`, `max_media`, `media` (MIME types, `type/*`), `question_kinds`, `wide_ids`, `yesno_descriptions`, `probabilities`, `locality` |

Field names are snake_case in every language, JS included: a dialect is a
portable JSON document, the same file in Rust, Python and JS.

Keys starting with `_` are comments (`_doc`). Unknown keys are an error, so a
typo such as `pathh` fails with `unknown field \`pathh\`` when the dialect is
registered. In JS that needs the core loaded: `await initialize()` before
creating the client, or the check happens on first use. `resolveDialect`
checks one up front.

### Media encodings

Each is tested code in the core; a dialect picks one by name.

| Name | Used by | What it sends |
|---|---|---|
| `none` | custom System One servers | no media |
| `jev_assets` | Jev | an `assets` map plus placeholders appended to the input; `format` from the MIME subtype; Jev's image gate header |
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
  "media_encoding": "none", "capabilities": { "media": [] } }

// System One behind a different key header, e.g. Microsoft-Decision-1 on Foundry
// (reported: {endpoint}/providers/microsoft/v1/systemone, `api-key` header)
{ "extends": "systemone", "base_url": "https://YOUR-RESOURCE.services.ai.azure.com/providers/microsoft",
  "auth": { "header": "api-key", "required": true } }

// An OpenAI-compatible endpoint, e.g. Azure OpenAI Decisions
{ "extends": "openai", "base_url": "https://YOUR-RESOURCE.openai.azure.com/openai",
  "auth": { "header": "api-key", "required": true } }
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
