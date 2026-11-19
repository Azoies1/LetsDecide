# 0003: `media` replaces `images` (breaking)

- Status: Accepted
- Date: 2026-11-19

## Context
Gemini accepts audio, video and PDF as well as images. URL handling differs per
provider: Clef refuses URLs, OpenAI and Jev need data or assets.

## Decision
One `media` list of `{ mime_type, data }`. The kind comes from the MIME type.
Inline base64 only, because it is the one form every provider accepts.
Providers that cannot take a kind return `Unsupported`.

## Consequences
- Portable and simple. Breaking change from `images` (see CHANGELOG).
- Per-media options (resolution, fps, page ranges) and URL inputs need a richer
  shape later (tracked in `future-work.md`).
