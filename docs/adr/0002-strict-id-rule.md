# 0002: Strict id rule `[a-z0-9_]{1,64}`

- Status: Accepted
- Date: 2026-11-19

## Context
Question, option and level ids are chosen by the app and returned unchanged.
Providers allow different character sets (Gemini allows `[A-Za-z0-9_-]` for
questions and richer option keys).

## Decision
Enforce the strictest common form everywhere: lowercase letters, digits and
underscore, 1 to 64 characters.

## Consequences
- A request valid for one provider is valid for all, which keeps "switch with
  one string" true.
- Callers who want hyphens or capitals must map ids themselves. Revisit if
  callers need the wider set (tracked in `future-work.md`).
