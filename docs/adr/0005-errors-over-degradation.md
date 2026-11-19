# 0005: Unsupported features error, never degrade

- Status: Accepted
- Date: 2026-11-19

## Context
Providers have different limits (question counts, score levels, media kinds).
Silently truncating or emulating would hide differences and corrupt results.

## Decision
Return a typed `Error` with a stable `code` and a `retryable()` flag
(`Unsupported`, `InvalidRequest`, `Auth`, `RateLimited`, `ProviderUnavailable`,
`BadResponse`). Fallback is opt-in middleware in the host language and also
routes around `Unsupported`.

## Consequences
- Callers see exactly what a provider cannot do.
- Callers who want resilience must opt in to `retry` and `fallback`.
