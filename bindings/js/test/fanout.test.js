import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Client, LetsDecideError, compose, fanOut, mockEngine, retry, initialize } from "../index.js";

const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
await initialize(wasm);

const request = {
  input: "Charged twice.",
  questions: {
    billing: { kind: "yesno", instructions: "Is this about billing?" },
    team: { kind: "choice", instructions: "Which team?", options: { pay: "Payments", tech: "Bugs" } },
  },
};

const says = (p) => mockEngine({ answers: { billing: { kind: "yesno", probability: p } } });
const down = { async decide() { throw new LetsDecideError({ code: "ProviderUnavailable", message: "x", retryable: true }); } };
const tiny = { capabilities: { max_questions: 1 }, decide: says(1).decide };
const slow = {
  async decide(m, r, o) {
    await new Promise((_, rej) => o.signal.addEventListener("abort", () => rej(o.signal.reason)));
  },
};

function client(extra = {}) {
  return new Client({}, { wasm, engines: { yes: says(0.9), no: says(0.1), down, tiny, slow, ...extra } });
}
const decide = (c) => c.decide.bind(c);

test("returns every member's raw result, keyed and in member order", async () => {
  const out = await fanOut(decide(client()), request, ["no/a", "yes/b", "down/c", "tiny/d"]);
  assert.deepEqual(Object.keys(out), ["no/a", "yes/b", "down/c", "tiny/d"]);
  assert.equal(out["yes/b"].response.answers.billing.probability, 0.9);
  assert.equal(out["no/a"].response.answers.billing.probability, 0.1);
  assert.equal(out["down/c"].error.code, "ProviderUnavailable");
  assert.equal(out["tiny/d"].skipped, true);
  assert.ok(Object.values(out).every((r) => typeof r.ms === "number"));
});

test("disabled members are not called", async () => {
  let called = false;
  const spy = { async decide() { called = true; } };
  const out = await fanOut(decide(client({ spy })), request, [{ model: "yes/a" }, { model: "spy/b", enabled: false }]);
  assert.equal(called, false);
  assert.deepEqual(Object.keys(out), ["yes/a"]);
});

test("routing sends each member only its questions", async () => {
  const seen = {};
  const spy = (name) => ({ async decide(m, r, o) { seen[name] = Object.keys(r.questions); return says(0.5).decide(m, r, o); } });
  const out = await fanOut(decide(client({ a: spy("a"), b: spy("b") })), request, [
    { model: "a/x", questions: ["billing"] },
    { model: "b/x", questions: ["team"] },
  ]);
  assert.deepEqual(seen, { a: ["billing"], b: ["team"] });
  assert.deepEqual(Object.keys(out["a/x"].response.answers), ["billing"]);
});

test("timeoutMs returns what arrived and aborts the rest", async () => {
  const t0 = Date.now();
  const out = await fanOut(decide(client()), request, ["yes/a", "slow/b"], { timeoutMs: 50 });
  assert.ok(Date.now() - t0 < 2000);
  assert.ok(out["yes/a"].response);
  assert.equal(out["slow/b"].timedOut, true);
});

test("works over composed middleware, per member", async () => {
  let calls = 0;
  const flaky = { async decide(m, r, o) { if (calls++ === 0) throw new LetsDecideError({ code: "RateLimited", message: "x", retryable: true }); return says(0.7).decide(m, r, o); } };
  const out = await fanOut(compose(client({ flaky }), retry({ sleep: async () => {} })), request, ["flaky/a"]);
  assert.equal(out["flaky/a"].response.attempts, 2);
});

test("the caller's abort is thrown, not reported", async () => {
  const ac = new AbortController();
  const p = fanOut(decide(client()), request, ["slow/a"], { signal: ac.signal });
  ac.abort(new Error("stop"));
  await assert.rejects(p, /stop/);
});

test("bad member lists throw before any call", async () => {
  let called = false;
  const spy = { async decide() { called = true; } };
  const d = decide(client({ spy }));
  for (const members of [
    [],
    [{ model: "spy/a", enabled: false }],
    ["spy/a", "spy/a"],
    [{ model: "spy/a", questions: ["nope"] }],
    [{ model: "spy/a", questions: [] }],
    [{ model: "spy/a", questions: ["billing"] }], // nobody is asked `team`
  ]) {
    await assert.rejects(fanOut(d, request, members), RangeError);
  }
  assert.equal(called, false);
});
