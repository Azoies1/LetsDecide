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

// Worth trying another call (or model) after this error.
const recoverable = (e) => e.retryable || e.code === "Unsupported";

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
        if (i + 1 >= chain.length || !recoverable(e)) throw Object.assign(e, { attempts });
      }
    }
  };
}

/**
 * Starts another call when the current one is slower than `delayMs` (or fails
 * recoverably); the first answer wins and the rest are aborted. Hedges go to
 * `models` in order, or to the same model when empty.
 */
export function hedge({ delayMs = 1000, models = [] } = {}) {
  return (next) => (model, request, opts = {}) =>
    new Promise((resolve, reject) => {
      const chain = [model, ...(models.length ? models : [model])];
      const ctrls = [];
      let started = 0, pending = 0, done = false, timer;
      const finish = (settle, value) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        opts.signal?.removeEventListener("abort", onAbort);
        ctrls.forEach((c) => c.abort());
        settle(value);
      };
      const onAbort = () => finish(reject, opts.signal.reason);
      if (opts.signal?.aborted) return reject(opts.signal.reason);
      opts.signal?.addEventListener("abort", onAbort, { once: true });
      const launch = () => {
        if (done || started >= chain.length) return;
        const i = started++;
        pending++;
        const ctrl = new AbortController();
        ctrls.push(ctrl);
        clearTimeout(timer);
        if (started < chain.length) timer = setTimeout(launch, delayMs);
        Promise.resolve()
          .then(() => next(chain[i], request, { ...opts, signal: ctrl.signal }))
          .then(
            (res) => finish(resolve, { ...res, hedgedFrom: i ? model : undefined }),
            (e) => {
              pending--;
              if (!recoverable(e)) return finish(reject, e);
              if (started < chain.length) return launch();
              if (!pending) finish(reject, e);
            },
          );
      };
      launch();
    });
}

/** Known per-request question caps, by provider. */
const QUESTION_LIMITS = { clef: 64, openai: 400, gemini: 1000 };

/**
 * Splits a request with more than `maxQuestions` questions into several and
 * merges the answers. `maxQuestions` is a number or `(model) => number`; by
 * default it is the provider's documented cap. Input tokens and cost are
 * summed: the input is sent (and billed) once per part.
 */
export function split({ maxQuestions, concurrency = 4 } = {}) {
  const limit = (model) =>
    (typeof maxQuestions === "function" ? maxQuestions(model) : maxQuestions) ??
    QUESTION_LIMITS[model.split("/")[0]] ??
    Infinity;
  return (next) => async (model, request, opts = {}) => {
    const ids = Object.keys(request.questions);
    const size = limit(model);
    if (ids.length <= size) return next(model, request, opts);
    const parts = [];
    for (let i = 0; i < ids.length; i += size) {
      const questions = Object.fromEntries(ids.slice(i, i + size).map((id) => [id, request.questions[id]]));
      parts.push({ ...request, questions });
    }
    // One failed part fails the whole call; abort the others.
    const ctrl = new AbortController();
    const onAbort = () => ctrl.abort(opts.signal.reason);
    opts.signal?.addEventListener("abort", onAbort, { once: true });
    const results = new Array(parts.length);
    let cursor = 0;
    const worker = async () => {
      while (cursor < parts.length) {
        const n = cursor++;
        results[n] = await next(model, parts[n], { ...opts, signal: ctrl.signal });
      }
    };
    try {
      await Promise.all(Array.from({ length: Math.min(concurrency, parts.length) }, worker));
    } catch (e) {
      ctrl.abort(e);
      throw e;
    } finally {
      opts.signal?.removeEventListener("abort", onAbort);
    }
    return merge(results);
  };
}

function merge(results) {
  const sum = (k) => (results.every((r) => r[k] != null) ? results.reduce((a, r) => a + r[k], 0) : null);
  const out = {
    model: results.find((r) => r.model)?.model ?? null,
    answers: Object.assign({}, ...results.map((r) => r.answers)),
    input_tokens: sum("input_tokens"),
    parts: results.length,
  };
  const cost = sum("estimated_cost_usd");
  if (cost != null) out.estimated_cost_usd = cost;
  const reason = results.find((r) => r.block_reason)?.block_reason;
  if (reason) out.block_reason = reason;
  if (results.some((r) => r.attempts)) out.attempts = results.reduce((a, r) => a + (r.attempts ?? 1), 0);
  return out;
}

/**
 * Also sends each request to `model` in the background and reports both
 * answers to `onResult({ model, shadowModel, request, primary, shadow, error, comparison })`.
 * The caller gets the primary answer as soon as it is ready; shadow
 * failures are reported, never thrown. `sample` (0..1) limits the share of traffic.
 */
export function shadow({ model: shadowModel, onResult, sample = 1, random = Math.random }) {
  return (next) => async (model, request, opts = {}) => {
    const run = random() < sample;
    const shadowCall = run
      ? Promise.resolve()
          .then(() => next(shadowModel, request, {}))
          .then((shadow) => ({ shadow }), (error) => ({ error }))
      : null;
    const primary = await next(model, request, opts);
    shadowCall?.then(({ shadow, error }) => {
      const comparison = shadow ? compare(primary, shadow) : undefined;
      try {
        onResult({ model, shadowModel, request, primary, shadow, error, comparison });
      } catch {
        // A reporting bug must not surface on the caller's path.
      }
    });
    return primary;
  };
}

/**
 * Per-question agreement between two responses. Yes/no agrees when both are
 * on the same side of 0.5; choice when the choice is equal; score when the
 * values round to the same level. `delta` is the absolute difference in
 * P(yes) or score. `agreement` is the share of comparable questions that agree.
 */
export function compare(a, b) {
  const questions = {};
  for (const [id, x] of Object.entries(a.answers)) {
    const y = b.answers[id];
    if (!y || x.kind !== y.kind || x.kind === "refused") continue;
    if (x.kind === "yesno") {
      questions[id] = { agree: x.probability >= 0.5 === y.probability >= 0.5, delta: Math.abs(x.probability - y.probability) };
    } else if (x.kind === "choice") {
      questions[id] = { agree: x.choice === y.choice };
    } else {
      questions[id] = { agree: Math.round(x.value) === Math.round(y.value), delta: Math.abs(x.value - y.value) };
    }
  }
  const all = Object.values(questions);
  const agreement = all.length ? all.filter((q) => q.agree).length / all.length : null;
  return { agreement, questions };
}

/**
 * Adds a `calibrated` field next to each raw answer; raw numbers are never
 * changed. `calibrators` is one `(p) => p` for every question or
 * `{ [questionId]: (p) => p }`. Yes/no gets `calibrated.probability`;
 * choice and score get renormalised `calibrated.probabilities` and the top
 * `calibrated.confidence`.
 */
export function calibrate(calibrators) {
  const pick = (id) => (typeof calibrators === "function" ? calibrators : calibrators[id]);
  return (next) => async (model, request, opts) => {
    const res = await next(model, request, opts);
    const answers = {};
    for (const [id, a] of Object.entries(res.answers)) {
      const f = pick(id);
      answers[id] = f && a.kind !== "refused" ? { ...a, calibrated: calibrateAnswer(a, f) } : a;
    }
    return { ...res, answers };
  };
}

function calibrateAnswer(a, f) {
  const clamp = (p) => Math.min(1, Math.max(0, f(p)));
  if (a.kind === "yesno") return { probability: clamp(a.probability) };
  const raw = Object.entries(a.probabilities).map(([k, p]) => [k, clamp(p)]);
  const total = raw.reduce((s, [, p]) => s + p, 0);
  const probabilities = Object.fromEntries(raw.map(([k, p]) => [k, total ? p / total : 0]));
  return { probabilities, confidence: Math.max(0, ...Object.values(probabilities)) };
}

/**
 * Fits a monotonic calibrator (isotonic regression, pool-adjacent-violators)
 * from `[predicted, outcome]` pairs, outcome 0 or 1. Returns `(p) => p`,
 * linear between fitted points.
 */
export function fitIsotonic(samples) {
  const sorted = [...samples].sort((a, b) => a[0] - b[0]);
  // Equal predictions form one point.
  const points = [];
  for (const [x, y] of sorted) {
    if (points.at(-1)?.x === x) {
      points.at(-1).sum += Number(y);
      points.at(-1).n++;
    } else points.push({ x, sum: Number(y), n: 1 });
  }
  const blocks = [];
  for (const p of points) {
    blocks.push({ ...p });
    while (blocks.length > 1 && mean(blocks.at(-2)) > mean(blocks.at(-1))) {
      const b = blocks.pop();
      const a = blocks.at(-1);
      a.x = (a.x * a.n + b.x * b.n) / (a.n + b.n);
      a.sum += b.sum;
      a.n += b.n;
    }
  }
  const pts = blocks.map((b) => [b.x, mean(b)]);
  return (p) => {
    if (!pts.length) return p;
    if (p <= pts[0][0]) return pts[0][1];
    for (let i = 1; i < pts.length; i++) {
      const [x0, y0] = pts[i - 1];
      const [x1, y1] = pts[i];
      if (p <= x1) return x1 === x0 ? y1 : y0 + ((p - x0) / (x1 - x0)) * (y1 - y0);
    }
    return pts.at(-1)[1];
  };
}
const mean = (b) => b.sum / b.n;

/** Mean squared error of `[predicted, outcome]` pairs: lower is better calibrated. */
export function brierScore(samples) {
  if (!samples.length) return null;
  return samples.reduce((s, [p, y]) => s + (p - y) ** 2, 0) / samples.length;
}

/**
 * Calls `onStart({ model, request })`, then `onEnd({ model, request, response, durationMs })`
 * or `onError({ model, request, error, durationMs })`. Hook errors are ignored.
 */
export function observe({ onStart, onEnd, onError, now = () => performance.now() } = {}) {
  const call = (hook, arg) => {
    try {
      hook?.(arg);
    } catch {
      // Observability must never change the result.
    }
  };
  return (next) => async (model, request, opts) => {
    const t0 = now();
    call(onStart, { model, request });
    try {
      const response = await next(model, request, opts);
      call(onEnd, { model, request, response, durationMs: now() - t0 });
      return response;
    } catch (error) {
      call(onError, { model, request, error, durationMs: now() - t0 });
      throw error;
    }
  };
}
