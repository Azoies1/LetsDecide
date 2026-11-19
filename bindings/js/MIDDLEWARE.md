# Middleware (opt-in)

A middleware is `(next) => (model, request, opts) => Promise<DecisionResponse>`.
`Client.decide` is the base `next`; `Client` itself is unchanged.

```js
import { Client } from "letsdecide";
import { compose, retry, fallback } from "letsdecide/middleware.js";

const decide = compose(client, fallback(["jev/jev-latest"]), retry({ maxAttempts: 3 }));
const res = await decide("clef/clef-flash", request, { signal });
res.attempts;     // calls made, failed ones included
res.fellBackFrom; // "clef/clef-flash" if jev answered, else undefined
```

The first middleware is outermost: above, each model gets its own retries.

- `retry({ maxAttempts = 3, baseMs = 200, sleep })`: retries `retryable` errors with jittered exponential backoff; `retryAfterMs` wins when present. Stops on `opts.signal` abort.
- `fallback(models)`: moves to the next model on a retryable error or `Unsupported` (so capability routing comes free). The last error is thrown if all fail.

Next: hedging, split/merge, shadow, calibration, observe.
