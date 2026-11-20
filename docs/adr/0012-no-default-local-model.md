# 0012: No default local model

- Status: Accepted
- Date: 2026-11-20

## Context
Local model quality varies by task, size, licence and hardware. A default
would age quickly and imply a recommendation we can't stand behind.

## Decision
The library names no default model; consumers choose. Fixtures use recorded
log-probs, not a specific model. Library-assisted choice (a decision that
routes between models, a bake-off on the consumer's own data) is future work.

## Consequences
- Docs show how to plug a model in, not which one.
- First-time local setup takes one more decision from the consumer.
