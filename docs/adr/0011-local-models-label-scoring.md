# 0011: Local models read label probabilities; never parse generated text

- Status: Accepted
- Date: 2026-11-20

## Context
Local runtimes (llama.cpp, vLLM, MLX, ONNX, transformers.js) run language
models, not decision APIs. A model can be asked to *write* a probability, or
its next-token distribution over answer labels can be *read*.

## Decision
Local decisions use label scoring: present labelled options, read the
probabilities of the label tokens, renormalise. Scores use the same 0-based
expected level index as Jev, Clef and Gemini (ADR 0004). We do not parse
generated text. Runtimes that expose no log-probs return `Unsupported`
(`no_logprobs`); there is no sampling mode. Responses say where numbers came
from (`probability_source`: `provider`, `logprob`, `nli`).

## Consequences
- A generated "0.8" is text, not a measurement; not using it keeps the
  "numbers are real" promise. Label scoring is deterministic and one forward
  pass instead of many decoding steps.
- Some on-device runtimes without log-probs can't be used until they expose
  them.
- Labels must be single tokens; more than about 26 options is `Unsupported`.
