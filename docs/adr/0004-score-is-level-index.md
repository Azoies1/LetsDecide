# 0004: Scores are 0-based level indexes

- Status: Accepted
- Date: 2026-11-19

## Context
Providers return a probability per score level. A single number is more useful
to callers.

## Decision
For Jev, Clef and Gemini, `value` is the probability-weighted 0-based level
index (for example 0.75 sits between level 0 and level 1). OpenAI's `score` is
passed through as sent. Provider numbers are never rescaled.

## Consequences
- Honest and reproducible from `probabilities`.
- Values are not comparable across providers. Making confidence comparable is
  an explicit non-goal.
