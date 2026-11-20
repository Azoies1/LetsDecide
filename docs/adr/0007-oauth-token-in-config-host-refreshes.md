# 0007: OAuth token goes in `Config`; the host refreshes it

- Status: Accepted
- Date: 2026-11-19

## Context
Clef is reached through Cloudflare, which accepts an OAuth access token as a
Bearer credential. Gemini uses a static key. The core is sans-IO (ADR 0001), so
it cannot run an OAuth flow or refresh an expired token.

## Decision
`Config.access_token` carries the current token and is sent as
`Authorization: Bearer`; it wins over `api_key`. Bindings accept a function
(sync or async) that is called on every request, so the host owns the OAuth flow,
caching and refresh. Gemini requires `api_key`; Clef requires `api_key` or
`access_token`; a missing credential fails with `InvalidRequest` before sending.

## Consequences
- The core stays pure and tokens never persist in the library (ADR 0006).
- Consumers must run their own OAuth flow. Helpers for the flow itself
  (authorize URL, PKCE, token exchange as pure build/parse functions) are not
  built; add them if callers need it.
