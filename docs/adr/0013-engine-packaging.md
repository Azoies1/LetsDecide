# 0013: Model runtimes ship in separate packages

- Status: Accepted
- Date: 2026-11-20

## Context
The core is small (two dependencies). Model runtimes are tens to hundreds of
MB with native libraries.

## Decision
Anything that pulls in a model runtime is a separate package: Python extras
(`letsdecide[llamacpp]`, `[transformers]`, `[onnx]`, lazy-imported), npm
`@letsdecide/transformers` and `@letsdecide/webllm`, separate SwiftPM products
and Gradle artifacts. Engine slots, the registry, the response check, the
`mock` engine, the label scorer, middleware and HTTP runtime shims (llama.cpp
server, vLLM/SGLang, Ollama) stay in the main packages. `letsdecide-serve` is
its own binary and container image.

## Consequences
- Main packages stay as small as today; server and PC users get local models
  over HTTP with no new dependencies.
- More packages to release and version together.
