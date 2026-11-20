import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Client, LetsDecideError, mockEngine, panelEngine, initialize } from "../index.js";

const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
await initialize(wasm);

const request = {
  input: "Charged twice.",
  questions: {
    billing: { kind: "yesno", instructions: "Is this about billing?" },
    team: { kind: "choice", instructions: "Which team?", options: { pay: "Payments", tech: "Bugs" } },
  },
};

const says = (p, team, source = "provider") =>
  mockEngine({
    probabilitySource: source,
    answers: {
      billing: { kind: "yesno", probability: p },
      team: { kind: "choice", choice: team, confidence: null, probabilities: { pay: team === "pay" ? 0.9 : 0.1, tech: team === "tech" ? 0.9 : 0.1 } },
    },
  });

const failing = (code, retryable = false) => ({
  async decide() {
    throw new LetsDecideError({ code, message: "nope", retryable });
  },
});
const slow = (ms, inner) => ({
  async decide(m, r, o) {
    await new Promise((res, rej) => {
      const t = setTimeout(res, ms);
      o.signal?.addEventListener("abort", () => (clearTimeout(t), rej(o.signal.reason)), { once: true });
    });
    return inner.decide(m, r, o);
  },
});
const tiny = { capabilities: { max_questions: 1 }, decide: says(1, "pay").decide };

function client(panels, extra = {}) {
  return new Client({}, {
    wasm,
    engines: {
      panel: panelEngine(panels),
      yes: says(0.9, "pay"),
      no: says(0.2, "tech"),
      logp: says(0.6, "pay", "logprob"),
      down: failing("ProviderUnavailable", true),
      tiny,
      ...extra,
    },
  });
}

test("vote merges members and reports agreement", async () => {
  const c = client({ p: { members: [{ model: "yes/a" }, { model: "no/b", weight: 2 }] } });
  const res = await c.decide("panel/p", request);
  assert.equal(res.answers.team.choice, "tech");
  assert.equal(res.probability_source, "vote");
  assert.ok(Math.abs(res.answers.billing.probability - 1 / 3) < 1e-6);
  assert.deepEqual(res.disagreed, ["billing", "team"]);
  assert.ok(res.panel["yes/a"].response && res.panel["no/b"].response);
});

test("disabled members are not called", async () => {
  let called = false;
  const spy = { async decide() { called = true; throw new Error("unreachable"); } };
  const c = client({ p: { members: [{ model: "yes/a" }, { model: "spy/x", enabled: false }] } }, { spy });
  const res = await c.decide("panel/p", request);
  assert.equal(called, false);
  assert.deepEqual(Object.keys(res.panel), ["yes/a"]);
  assert.deepEqual(res.disagreed, []);
});

test("config getter is read on every call", async () => {
  let members = [{ model: "yes/a" }];
  const c = client(() => ({ p: { members } }));
  assert.equal((await c.decide("panel/p", request)).answers.team.choice, "pay");
  members = [{ model: "no/b" }];
  assert.equal((await c.decide("panel/p", request)).answers.team.choice, "tech");
});

test("members that can't take the request are skipped, not failed", async () => {
  const c = client({ p: { members: [{ model: "tiny/a" }, { model: "no/b" }], rules: { quorum: 1 } } });
  const res = await c.decide("panel/p", request);
  assert.equal(res.panel["tiny/a"].skipped, true);
  assert.equal(res.answers.team.choice, "tech");
});

test("below quorum throws the member's error, still retryable", async () => {
  const c = client({ p: { members: [{ model: "yes/a" }, { model: "down/b" }], rules: { quorum: 2 } } });
  await assert.rejects(c.decide("panel/p", request), (e) => {
    assert.equal(e.code, "ProviderUnavailable");
    assert.equal(e.retryable, true);
    assert.equal(e.details.reason, "below_quorum");
    assert.equal(e.details.provider, "down");
    assert.ok(e.panel["down/b"].error);
    return true;
  });
});

test("mean refuses mixed probability sources", async () => {
  const c = client({ p: { members: [{ model: "yes/a" }, { model: "logp/b" }], rules: { merge: "mean" } } });
  await assert.rejects(c.decide("panel/p", request), (e) => e.details?.reason === "mixed_probability_source");
});

test("timeout_ms merges what arrived and records the late member", async () => {
  const c = client(
    { p: { members: [{ model: "yes/a" }, { model: "late/b" }], rules: { timeout_ms: 50 } } },
    { late: slow(5000, says(0.1, "tech")) },
  );
  const t0 = Date.now();
  const res = await c.decide("panel/p", request);
  assert.ok(Date.now() - t0 < 2000);
  assert.equal(res.answers.team.choice, "pay");
  assert.equal(res.panel["late/b"].error.code, "ProviderUnavailable");
});

test("race returns the first answer and aborts the rest", async () => {
  let aborted = false;
  const late = {
    async decide(m, r, o) {
      await new Promise((_, rej) => o.signal.addEventListener("abort", () => ((aborted = true), rej(o.signal.reason))));
    },
  };
  const c = client({ p: { mode: "race", members: [{ model: "late/a" }, { model: "down/b" }, { model: "no/c" }] } }, { late });
  const res = await c.decide("panel/p", request);
  assert.equal(res.answers.team.choice, "tech");
  assert.equal(res.probability_source, "provider");
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(aborted, true);
});

test("race with no answer throws the last failure", async () => {
  const c = client({ p: { mode: "race", members: [{ model: "tiny/a" }, { model: "down/b" }] } });
  await assert.rejects(c.decide("panel/p", request), (e) => e.code === "ProviderUnavailable" && e.panel["tiny/a"].skipped);
});

test("bad config is InvalidRequest with a field", async () => {
  const c = client({ empty: { members: [{ model: "yes/a", enabled: false }] }, odd: { mode: "all", members: [{ model: "yes/a" }] } });
  for (const [name, field] of [["missing", "panels"], ["empty", "members"], ["odd", "mode"]]) {
    await assert.rejects(c.decide(`panel/${name}`, request), (e) => e.code === "InvalidRequest" && e.details.field === field);
  }
});

test("a panel listing itself stops at MAX_DEPTH", async () => {
  const c = client({ p: { members: [{ model: "panel/p" }] } });
  // The innermost call fails on depth; each level above reports it as below quorum.
  await assert.rejects(c.decide("panel/p", request), (e) => e.code === "InvalidRequest" && e.details?.reason === "below_quorum");
});

test("engine extras pass through; checked fields win and are still checked", async () => {
  const base = { model: "m", input_tokens: null, answers: { billing: { kind: "yesno", probability: 0.5 }, team: { kind: "choice", choice: "pay", confidence: null, probabilities: {} } } };
  const extra = { async decide() { return { ...base, trace: "t1" }; } }; // no probability_source
  const bad = { async decide() { return { ...base, answers: { billing: base.answers.billing }, trace: "t1" }; } };
  const c = new Client({}, { wasm, engines: { extra, bad } });
  const res = await c.decide("extra/x", request);
  assert.equal(res.trace, "t1");
  assert.equal(res.probability_source, "provider"); // filled by the check, not the engine
  await assert.rejects(c.decide("bad/x", request), (e) => e.code === "BadResponse");
});

test("routing sends each member only its questions and votes per question", async () => {
  const seen = {};
  const spyOn = (name, inner) => ({ async decide(m, r, o) { seen[name] = Object.keys(r.questions); return inner.decide(m, r, o); } });
  const c = client(
    { p: { members: [{ model: "a/x", questions: ["billing"] }, { model: "b/x", questions: ["team"] }, { model: "no/c" }] } },
    { a: spyOn("a", says(0.9, "pay")), b: spyOn("b", says(0.9, "pay")) },
  );
  const res = await c.decide("panel/p", request);
  assert.deepEqual(seen, { a: ["billing"], b: ["team"] });
  assert.ok(Math.abs(res.answers.billing.probability - 0.5) < 1e-6); // a yes, no/c no
  assert.deepEqual(res.disagreed, ["billing", "team"]);
});

test("routing config is checked before any call", async () => {
  let called = false;
  const spy = { async decide() { called = true; throw new Error("unreachable"); } };
  const cases = {
    uncovered: [{ model: "spy/a", questions: ["billing"] }],
    unknown: [{ model: "spy/a", questions: ["nope"] }, { model: "yes/b" }],
  };
  const c = client({ ...Object.fromEntries(Object.entries(cases).map(([k, members]) => [k, { members }])), race: { mode: "race", members: [{ model: "spy/a", questions: ["billing"] }, { model: "yes/b" }] } }, { spy });
  for (const [name, field] of [["uncovered", "members"], ["unknown", "questions"], ["race", "questions"]]) {
    await assert.rejects(c.decide(`panel/${name}`, request), (e) => e.code === "InvalidRequest" && e.details.field === field);
  }
  assert.equal(called, false);
});

test("routing quorum is per question", async () => {
  const c = client({ p: { members: [{ model: "down/a", questions: ["billing"] }, { model: "yes/b", questions: ["team"] }] } });
  await assert.rejects(c.decide("panel/p", request), (e) => e.details.reason === "below_quorum" && e.details.question === "billing" && e.details.provider === "down");
});
