import test from "node:test";
import assert from "node:assert/strict";
import {
  compose, hedge, split, shadow, compare, calibrate, fitIsotonic, brierScore, observe, retry,
} from "../middleware.js";

const err = (code, retryable = false) => Object.assign(new Error(code), { code, retryable });
const yes = (p) => ({ kind: "yesno", probability: p });
const q = (n) => Object.fromEntries(Array.from({ length: n }, (_, i) => [`q${i}`, { kind: "yesno", instructions: "?" }]));

// A fake client: `impl(model, request, opts)` decides; every call is recorded.
function fake(impl) {
  const calls = [];
  return { calls, decide: (model, request, opts) => (calls.push({ model, request, opts }), impl(model, request, opts, calls.length)) };
}
const wait = (ms, signal) =>
  new Promise((resolve, reject) => {
    const t = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => (clearTimeout(t), reject(err("Aborted"))));
  });
const answerAll = (model, request) => ({
  estimated_cost_usd: 0.5,
  model,
  answers: Object.fromEntries(Object.keys(request.questions).map((id) => [id, yes(0.9)])),
  input_tokens: 10,
});

test("hedge: a slow first call is overtaken and aborted", async () => {
  const c = fake(async (model, req, { signal }, n) => {
    await wait(n === 1 ? 200 : 5, signal);
    return answerAll(model, req);
  });
  const res = await compose(c, hedge({ delayMs: 20, models: ["gemini/b"] }))("jev/a", { questions: q(1) });
  assert.equal(res.model, "gemini/b");
  assert.equal(res.hedgedFrom, "jev/a");
  assert.equal(c.calls.length, 2);
  assert.ok(c.calls[0].opts.signal.aborted);
});

test("hedge: a fast answer starts no hedge", async () => {
  const c = fake(async (model, req) => answerAll(model, req));
  const res = await compose(c, hedge({ delayMs: 50 }))("jev/a", { questions: q(1) });
  assert.equal(res.hedgedFrom, undefined);
  await wait(80);
  assert.equal(c.calls.length, 1);
});

test("hedge: a recoverable failure launches the hedge at once; a hard one throws", async () => {
  const c = fake(async (model, req, _o, n) => {
    if (n === 1) throw err("ProviderUnavailable", true);
    return answerAll(model, req);
  });
  const res = await compose(c, hedge({ delayMs: 10_000 }))("jev/a", { questions: q(1) });
  assert.equal(res.hedgedFrom, "jev/a");

  const d = fake(async () => { throw err("Auth"); });
  await assert.rejects(compose(d, hedge({ delayMs: 10_000 }))("jev/a", { questions: q(1) }), { code: "Auth" });
  assert.equal(d.calls.length, 1);
});

test("hedge: all fail throws the last error", async () => {
  const c = fake(async () => { throw err("RateLimited", true); });
  await assert.rejects(compose(c, hedge({ delayMs: 5 }))("jev/a", { questions: q(1) }), { code: "RateLimited" });
  assert.equal(c.calls.length, 2);
});

test("split: over the provider cap is chunked and merged", async () => {
  const c = fake(async (model, req) => answerAll(model, req));
  const res = await compose(c, split())("clef/clef-flash", { input: "x", questions: q(130) });
  assert.equal(c.calls.length, 3);
  assert.deepEqual(c.calls.map((x) => Object.keys(x.request.questions).length), [64, 64, 2]);
  assert.equal(Object.keys(res.answers).length, 130);
  assert.equal(res.input_tokens, 30);
  assert.equal(res.parts, 3);
  assert.equal(res.estimated_cost_usd, 1.5);
});

test("split: under the cap passes through; custom limits; one failure fails all", async () => {
  const c = fake(async (model, req) => answerAll(model, req));
  const res = await compose(c, split())("jev/x", { questions: q(500) });
  assert.equal(c.calls.length, 1);
  assert.equal(res.parts, undefined);

  const d = fake(async (model, req) => answerAll(model, req));
  await compose(d, split({ maxQuestions: (m) => (m.startsWith("jev") ? 2 : 100) }))("jev/x", { questions: q(5) });
  assert.equal(d.calls.length, 3);

  const e = fake(async (model, req, _o, n) => {
    if (n === 2) throw err("Auth");
    return answerAll(model, req);
  });
  await assert.rejects(compose(e, split({ maxQuestions: 1, concurrency: 1 }))("jev/x", { questions: q(3) }), { code: "Auth" });
  assert.equal(e.calls.length, 2);
});

test("shadow: primary returned, both reported with a comparison; shadow errors swallowed", async () => {
  const reports = [];
  const c = fake(async (model, req) =>
    model === "jev/a" ? answerAll(model, req) : { model, answers: { q0: yes(0.2) }, input_tokens: 1 },
  );
  const res = await compose(c, shadow({ model: "gemini/b", onResult: (r) => reports.push(r) }))("jev/a", { questions: q(1) });
  assert.equal(res.model, "jev/a");
  await wait(5);
  assert.equal(reports.length, 1);
  assert.equal(reports[0].shadow.model, "gemini/b");
  assert.equal(reports[0].comparison.agreement, 0);

  const d = fake(async (model, req) => {
    if (model === "gemini/b") throw err("Auth");
    return answerAll(model, req);
  });
  const out = await compose(d, shadow({ model: "gemini/b", onResult: (r) => reports.push(r) }))("jev/a", { questions: q(1) });
  assert.equal(out.model, "jev/a");
  await wait(5);
  assert.equal(reports[1].error.code, "Auth");

  const e = fake(async (model, req) => answerAll(model, req));
  await compose(e, shadow({ model: "gemini/b", onResult() {}, sample: 0.5, random: () => 0.9 }))("jev/a", { questions: q(1) });
  assert.equal(e.calls.length, 1);
});

test("compare: per kind", () => {
  const a = { answers: { y: yes(0.8), c: { kind: "choice", choice: "a" }, s: { kind: "score", value: 1.2 }, r: { kind: "refused" } } };
  const b = { answers: { y: yes(0.6), c: { kind: "choice", choice: "b" }, s: { kind: "score", value: 0.9 }, r: { kind: "refused" } } };
  const out = compare(a, b);
  assert.equal(out.questions.y.agree, true);
  assert.ok(Math.abs(out.questions.y.delta - 0.2) < 1e-9);
  assert.equal(out.questions.c.agree, false);
  assert.equal(out.questions.s.agree, true);
  assert.equal(out.questions.r, undefined);
  assert.equal(out.agreement, 2 / 3);
});

test("calibrate: adds calibrated values, raw untouched", async () => {
  const c = fake(async () => ({
    answers: {
      y: yes(0.8),
      c: { kind: "choice", choice: "a", confidence: 0.6, probabilities: { a: 0.6, b: 0.4 } },
      r: { kind: "refused" },
    },
  }));
  const res = await compose(c, calibrate((p) => p / 2))("jev/a", {});
  assert.equal(res.answers.y.probability, 0.8);
  assert.equal(res.answers.y.calibrated.probability, 0.4);
  assert.deepEqual(res.answers.c.calibrated.probabilities, { a: 0.6, b: 0.4 });
  assert.equal(res.answers.c.calibrated.confidence, 0.6);
  assert.equal(res.answers.r.calibrated, undefined);

  const only = await compose(c, calibrate({ y: () => 2 }))("jev/a", {});
  assert.equal(only.answers.y.calibrated.probability, 1);
  assert.equal(only.answers.c.calibrated, undefined);
});

test("fitIsotonic is monotonic and improves Brier on miscalibrated data", () => {
  // The model says 0.9 but is right half the time; 0.1 is right never.
  const samples = [];
  for (let i = 0; i < 50; i++) samples.push([0.9, i % 2], [0.1, 0]);
  const f = fitIsotonic(samples);
  assert.ok(Math.abs(f(0.9) - 0.5) < 1e-9);
  assert.equal(f(0.1), 0);
  assert.ok(f(0.5) >= f(0.1) && f(0.5) <= f(0.9));
  assert.ok(brierScore(samples.map(([p, y]) => [f(p), y])) < brierScore(samples));
  assert.equal(brierScore([]), null);
});

test("observe: hooks see timing, results and errors; hook errors are ignored", async () => {
  const seen = [];
  let t = 0;
  const hooks = {
    now: () => (t += 5),
    onStart: (e) => seen.push(["start", e.model]),
    onEnd: (e) => { seen.push(["end", e.durationMs]); throw new Error("hook bug"); },
    onError: (e) => seen.push(["error", e.error.code]),
  };
  const ok = fake(async (model, req) => answerAll(model, req));
  const res = await compose(ok, observe(hooks))("jev/a", { questions: q(1) });
  assert.equal(res.model, "jev/a");
  const bad = fake(async () => { throw err("Auth"); });
  await assert.rejects(compose(bad, observe(hooks), retry())("jev/a", {}), { code: "Auth" });
  assert.deepEqual(seen, [["start", "jev/a"], ["end", 5], ["start", "jev/a"], ["error", "Auth"]]);
});

test("review fixes: split honours a pre-aborted signal and rejects bad options", async () => {
  const c = fake(async (model, req) => answerAll(model, req));
  const ac = new AbortController();
  ac.abort(new Error("gone"));
  await assert.rejects(compose(c, split())("clef/x", { questions: q(100) }, { signal: ac.signal }), /gone/);
  assert.equal(c.calls.length, 0);
  assert.throws(() => split({ concurrency: 0 }), RangeError);
  await assert.rejects(compose(c, split({ maxQuestions: () => 0 }))("jev/x", { questions: q(3) }), RangeError);
});

test("review fixes: a hard hedge error waits for the call still in flight", async () => {
  const c = fake(async (model, req, { signal }, n) => {
    if (n === 2) throw err("InvalidRequest");
    await wait(40, signal);
    return answerAll(model, req);
  });
  const res = await compose(c, hedge({ delayMs: 10, models: ["gemini/b", "openai/c"] }))("clef/a", { questions: q(1) });
  assert.equal(res.model, "clef/a");
  assert.equal(c.calls.length, 2); // no further hedge after the hard error

  const d = fake(async (model, req, { signal }, n) => {
    if (n === 2) throw err("InvalidRequest");
    await wait(20, signal);
    throw err("ProviderUnavailable", true);
  });
  await assert.rejects(compose(d, hedge({ delayMs: 5, models: ["gemini/b"] }))("clef/a", { questions: q(1) }), { code: "InvalidRequest" });
});

test("review fixes: split merge reports mixed models and only full blocks", async () => {
  const c = fake(async (_m, req, _o, n) => ({
    model: n === 1 ? "clef-flash" : "jev-1",
    fellBackFrom: n === 2 ? "clef/clef-flash" : undefined,
    answers: Object.fromEntries(Object.keys(req.questions).map((id) => [id, n === 1 ? { kind: "refused" } : yes(0.5)])),
    input_tokens: 1,
    block_reason: n === 1 ? "SAFETY" : undefined,
  }));
  const res = await compose(c, split({ maxQuestions: 1, concurrency: 1 }))("clef/clef-flash", { questions: q(2) });
  assert.deepEqual(res.models, ["clef-flash", "jev-1"]);
  assert.equal(res.fellBackFrom, "clef/clef-flash");
  assert.equal(res.block_reason, undefined);
});

test("review fixes: calibrate ignores inherited keys; shadow is aborted when the primary fails", async () => {
  const c = fake(async () => ({ answers: { constructor: yes(0.8) } }));
  const res = await compose(c, calibrate({ other: (p) => p }))("jev/a", {});
  assert.equal(res.answers.constructor.calibrated, undefined);

  const reports = [];
  const d = fake(async (model, _r, { signal } = {}) => {
    if (model === "jev/a") throw err("Auth");
    await wait(1000, signal);
  });
  await assert.rejects(compose(d, shadow({ model: "gemini/b", onResult: (r) => reports.push(r) }))("jev/a", {}), { code: "Auth" });
  const shadowCall = d.calls.find((x) => x.model === "gemini/b");
  assert.ok(shadowCall.opts.signal.aborted);
  await wait(5);
  assert.equal(reports.length, 0);
});
