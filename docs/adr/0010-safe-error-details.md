# 0010: Structured error details that are safe to log

- Status: Accepted
- Date: 2026-11-20

## Context
`Unsupported` alone doesn't say what to change. Richer errors risk leaking
input, keys or provider replies into logs. serde's messages quote values
(`invalid type: string "…"`), which can be user input or a response body.

## Decision
`Error.details` carries a stable `reason` (snake_case) and only fields that
are safe by construction: provider, question/option ids, limit and actual
counts, MIME type, config field *name*. serde messages that could quote a
value are reduced to category and position. `Config`'s `Debug` redacts the
key, token, header values, option values and the `base_url` query string.
`ProviderError` is reserved for errors thrown by user code. Canary tests plant
a marker in every sensitive place and check no error contains it.

## Consequences
- Callers can branch on `reason` and log errors freely.
- Error text from user code (custom providers) is outside our control;
  bindings wrap it without serialising its message.
