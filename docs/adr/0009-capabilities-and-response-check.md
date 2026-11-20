# 0009: Capabilities as data; every response checked

- Status: Accepted
- Date: 2026-11-20

## Context
Limits lived as code in each adapter. Custom providers and capability-aware
routing need the same checks, as data. Custom providers are user code, so a
bug could hand the app a malformed answer.

## Decision
`Provider::capabilities(model)` returns limits, accepted media (`type/*`
patterns), question kinds, id rule, probability source and locality. The
registry checks a request against it before `build`. After `parse`,
`DecisionResponse::check` verifies every question is answered once, kinds and
ids match, and probabilities are finite and in `[0, 1]` (1e-3 slack). Score
`value` is only checked to be finite.

## Consequences
- Every provider, built-in or custom, gets the same guarantees.
- The check runs locally on the parsed reply: microseconds, no request, no
  tokens. It can't make a wrong answer right, only reject a malformed one.
- If a provider returns a malformed answer, the call fails with `BadResponse`
  after its tokens were spent; that is preferred to passing bad numbers on.
