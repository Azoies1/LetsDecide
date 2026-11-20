# Concurrent models: `fanOut`

Status: **built** (JS `fanOut`, Python `fan_out`).

Ask several decision models the same request at once and get every raw
answer back. The library stays a thin access layer, so it never combines the
answers. Voting, averaging or picking a winner is a policy choice, and it
belongs in the app (see [Combining answers](#combining-answers-yourself)).

## Usage

```js
import { Client, compose, retry, fanOut } from "letsdecide";

const client = new Client({ byProvider: { jev: { apiKey: JEV_KEY }, gemini: { apiKey: GEMINI_KEY } } });
const decide = compose(client, retry()); // middleware applies per model

const results = await fanOut(decide, request, [
  "jev/jev-latest",
  { model: "gemini/gemini-decision-1", questions: ["nsfw"] }, // routing
  { model: "openai/gpt-6-luna", enabled: false },             // switched off
], { timeoutMs: 3000 });

results["jev/jev-latest"]; // { response, ms } | { error, ms } | { skipped: true, ms } | { timedOut: true, ms }
```

```python
from letsdecide.middleware import fan_out

results = await fan_out(client.decide, request, ["jev/jev-latest", "gemini/gemini-decision-1"], timeout_ms=3000)
```

| Option | Meaning |
|---|---|
| `members` | `"provider/model"` strings or `{ model, enabled?, questions? }`. Plain data: read it from your own config or flags on every call to toggle models live |
| `enabled: false` | The model is not called and does not appear in the results |
| `questions` | Routing: that model gets a request with only these questions. Without it, a model gets all of them |
| `timeoutMs` / `timeout_ms` | Deadline for the whole call. Models still running are aborted and reported as `timedOut` |
| `signal` (JS) | The caller's abort is thrown, not reported. In Python, cancel the task |

- **Results** are keyed by model, in member order. Failures are returned, never
  thrown.
- **Skipped models:** a model that can't take the request (for example Clef
  with more than 64 questions) is reported as `skipped`. The client raises
  `Unsupported` before any network call, so a skip costs nothing.
- **Bad member lists fail before any call** (`RangeError` in JS, `ValueError`
  in Python): an empty list, a model listed twice, an unknown or empty
  `questions` list, or a question no model is asked.
- **It's built on `decide`,** so it works with `client.decide` or any composed
  middleware stack, and with custom providers and engines.
- **Keys:** credentials exist only per provider (`byProvider`), so one model's
  key is never sent to another.
- **Python** has it in `letsdecide.middleware` and needs `AsyncClient`, like
  the other middleware.
- **Need the fastest single answer?** Use `hedge({ delayMs: 0, models })`
  instead.

## Combining answers yourself

Probabilities are each provider's own numbers and are not comparable across
providers, so the library doesn't merge them. A majority vote on top answers
is a few lines in the app:

```js
const answers = Object.values(results).filter((r) => r.response).map((r) => r.response.answers.billing);
const yes = answers.filter((a) => a.kind === "yesno" && a.probability >= 0.5).length;
const billing = yes > answers.length / 2;
```

## Why not built in

An earlier version merged answers in the core (weighted vote, mean, quorum,
`probability_source: "vote"`). It was removed before release because:

- It put policy into a core whose job is translation.
- It created numbers no provider sent, which breaks "numbers are the
  provider's, never rescaled".
- It approached the non-goal "making confidence numbers comparable across
  providers".
- Nobody needed it yet.

If a real need appears, it can return as a separate package.
