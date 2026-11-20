# Middleware (opt-in)

A middleware is `(next) => (model, request, opts) => Promise<DecisionResponse>`.
`Client.decide` is the base `next`; `Client` itself is unchanged. Python has the
same set for `AsyncClient` (`letsdecide.middleware`, snake_case names).

```js
import { Client } from "letsdecide";
import { compose, retry, fallback, split, observe } from "letsdecide/middleware.js";

const decide = compose(client, observe({ onEnd: log }), split(), fallback(["jev/jev-latest"]), retry());
const res = await decide("clef/clef-flash", request, { signal });
res.attempts;     // calls made, failed ones included
res.fellBackFrom; // "clef/clef-flash" if jev answered, else undefined
```

The first middleware is outermost: above, each model gets its own retries.

## Reliability

- `retry({ maxAttempts = 3, baseMs = 200, maxMs = 30000, sleep })`: retries
  `retryable` errors with jittered exponential backoff; `retryAfterMs` wins
  when present. Stops on `opts.signal` abort.
- `fallback(models)`: moves to the next model on a retryable error or
  `Unsupported`, so capability routing comes free (a provider that can't take
  video falls through to one that can). The last error is thrown if all fail.
- `hedge({ delayMs = 1000, models = [] })`: if a call hasn't answered after
  `delayMs`, starts another (to `models` in order, or the same model). A
  recoverable failure starts the next one at once. The first answer wins;
  the others are aborted. `res.hedgedFrom` is set when a hedge answered.
  Hedging costs extra calls; use it for latency-critical paths.
- `split({ maxQuestions, concurrency = 4 })`: a request with more questions
  than the provider allows (Clef 64, OpenAI 400, Gemini 1000, or
  `maxQuestions`, a number or `(model) => number`) is sent in parts and
  merged. `input_tokens` is summed, since each part bills the input again.
  `res.parts` says how many. One failed part fails the call.

## Evaluation

- `shadow({ model, onResult, sample = 1 })`: mirrors traffic to `model` in the
  background. The caller gets the primary answer as soon as it is ready;
  `onResult({ primary, shadow, error, comparison, ... })` runs when the
  shadow answers. Shadow errors are reported, never thrown.
- `compare(a, b)`: per-question agreement. Yes/no agrees on the same side of
  0.5, choice on the same option, score on the same rounded level.
  `{ agreement, questions: { id: { agree, delta } } }`.
- `calibrate(fn | { id: fn })`: adds `answer.calibrated` (`probability`, or
  renormalised `probabilities` and `confidence`). Raw numbers are never
  changed. Confidence is still not comparable across providers.
- `fitIsotonic([[predicted, outcome], ...])`: fits a monotonic calibrator from
  labelled data (outcome 0 or 1). `brierScore(pairs)` measures the result.

```js
const f = fitIsotonic(labelled.map((x) => [x.probability, x.truth ? 1 : 0]));
const decide = compose(client, calibrate({ spam: f }));
(await decide(model, req)).answers.spam.calibrated.probability;
```

## Observability

- `observe({ onStart, onEnd, onError })`: hooks with `durationMs`. A hook that
  throws is ignored; it never changes the result.
