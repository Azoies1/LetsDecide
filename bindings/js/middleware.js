// A middleware is `(next) => (model, request, opts) => Promise<DecisionResponse>`.

/** Wraps `client.decide` in the given middleware; the first is outermost. */
export function compose(client, ...middleware) {
  return middleware.reduceRight((next, mw) => mw(next), (...args) => client.decide(...args));
}

const defaultSleep = (ms, signal) =>
  new Promise((resolve, reject) => {
    const t = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => (clearTimeout(t), reject(signal.reason)), { once: true });
  });

/** Retries `retryable` errors with jittered exponential backoff, honouring `retryAfterMs` (capped at `maxMs`). */
export function retry({ maxAttempts = 3, baseMs = 200, maxMs = 30_000, sleep = defaultSleep } = {}) {
  return (next) => async (model, request, opts = {}) => {
    for (let attempts = 1; ; attempts++) {
      opts.signal?.throwIfAborted();
      try {
        return { ...(await next(model, request, opts)), attempts };
      } catch (e) {
        if (!e.retryable || attempts >= maxAttempts) throw Object.assign(e, { attempts });
        const backoff = baseMs * 2 ** (attempts - 1) * (0.5 + Math.random() / 2);
        await sleep(Math.min(e.retryAfterMs ?? backoff, maxMs), opts.signal);
      }
    }
  };
}

/** On a retryable or `Unsupported` error, tries the next model; throws the last error if all fail. */
export function fallback(models) {
  return (next) => async (model, request, opts) => {
    const chain = [model, ...models];
    let attempts = 0;
    for (let i = 0; ; i++) {
      try {
        const res = await next(chain[i], request, opts);
        return { ...res, attempts: attempts + (res.attempts ?? 1), fellBackFrom: i ? model : undefined };
      } catch (e) {
        attempts += e.attempts ?? 1;
        if (i + 1 >= chain.length || !(e.retryable || e.code === "Unsupported")) throw Object.assign(e, { attempts });
      }
    }
  };
}
