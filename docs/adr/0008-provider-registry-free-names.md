# 0008: Provider registry with free names; a clash is an error

- Status: Accepted
- Date: 2026-11-20

## Context
Users need providers we don't ship (another cloud, an in-house service) without
forking. `resolve()` was a closed `match`. Custom names can clash with
built-ins, today or when we add one later.

## Decision
A `Registry` value (not a global) maps prefixes to providers; each client owns
one. Names are free, `[a-z0-9_-]{1,64}`. Registering a taken name, built-in or
custom, fails with `InvalidRequest` / `name_taken` unless the caller uses
`replace`. We rejected a reserved namespace such as `x-acme/`.

## Consequences
- Model strings stay readable (`acme/model`), which is the library's pitch.
- If we later ship a built-in with a user's name, their `register` fails after
  upgrading: loudly, at start-up, naming the clash. The changelog lists every
  new built-in prefix.
- A reserved namespace would have made that clash impossible; we judged the
  loud failure cheaper than uglier names.
