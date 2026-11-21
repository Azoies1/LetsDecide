# 0016: Thin access layer; no local-model runtime work

- Status: Accepted
- Date: 2026-11-21

## Context
Phases P4 to P6 of the [custom and local providers plan](../plan-custom-and-local-providers.md)
would have made the library run models itself: a label scorer over log-probs,
a `letsdecide-serve` sidecar, in-process engines, reference llama.cpp and ONNX
engines, and the middleware and mobile hooks that serve them. The library's
scope is a thin access layer: one request shape for decision APIs, with the
caller's own keys and HTTP stack.

## Decision
None of P4 to P6 is built.

- **Label scorer, sidecar, in-process and reference engines:** running models
  is not access. A local model that speaks System One is reached today with
  `systemone/<model>` and a `baseUrl`, or with a dialect.
- **`escalate`:** a policy, not access. It is a few lines over `decide`
  (check the answer, call a second model).
- **Capability-aware `fallback`:** `fallback` already moves on when a model
  raises `Unsupported`, which the core raises before sending.
- **Engine slots in Kotlin and Swift:** they exist to host on-device models.
  Other providers are covered by dialects, which those clients now have.

What already ships stays: custom codecs and engines in JS and Python, the
`mock` engine, `probability_source` and the conformance kit.

## Consequences
- The core keeps two dependencies and no model code.
- Callers who want local models run their own server, or write a codec or
  engine (JS, Python).
- Supersedes [0011](0011-local-models-label-scoring.md) and
  [0013](0013-engine-packaging.md). [0012](0012-no-default-local-model.md)
  still holds: we name no model.
- If a real need appears, a new ADR revisits this.
