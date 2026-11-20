import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { inspect } from "node:util";
import { BUILTIN, Client, LetsDecideError, classifyHttp, compose, fallback, mockEngine, initialize } from "../index.js";
import { checkProvider } from "../conformance.js";
import * as core from "../pkg/letsdecide_wasm.js";

const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
await initialize(wasm);

const request = {
  input: "Charged twice.",
  questions: {
    billing: { kind: "yesno", instructions: "Is this about billing?" },
    team: { kind: "choice", instructions: "Which team?", options: { billing: "Payments", tech: "Bugs" } },
  },
};

// A made-up vendor: answers as an array of { id, p } / { id, pick, probs }.
const acme = {
  capabilities: { max_questions: 5 },
  build(model, req, config) {
    return {
      url: `https://api.acme.test/v1/${model}`,
      headers: { authorization: `Bearer ${config.apiKey}`, "content-type": "application/json" },
      body: JSON.stringify({ text: req.input, ask: Object.keys(req.questions) }),
    };
  },
  async parse(req, res) {
    if (res.status >= 300) throw await classifyHttp(res);
    const body = JSON.parse(res.body);
    const answers = {};
    for (const a of body.results) {
      answers[a.id] = req.questions[a.id].kind === "yesno"
        ? { kind: "yesno", probability: a.p }
        : { kind: "choice", choice: a.pick, confidence: null, probabilities: a.probs };
    }
    return { model: body.model, answers, input_tokens: null };
  },
};
const acmeReply = {
  model: "acme-2",
  results: [
    { id: "billing", p: 0.9 },
    { id: "team", pick: "billing", probs: { billing: 0.7, tech: 0.3 } },
  ],
};
const respond = (status, body, headers = {}) => async () => new Response(JSON.stringify(body), { status, headers });

test("BUILTIN matches the core", () => {
  assert.deepEqual([...BUILTIN].sort(), JSON.parse(core.builtins()).ok.sort());
});

test("a custom codec gets config, headers, validation and response checks", async () => {
  let sent;
  const fetch = async (url, init) => ((sent = { url, ...init }), respond(200, acmeReply)());
  const client = new Client({ apiKey: "k", headers: { "x-gw": "g" } }, { fetch, wasm, providers: { acme } });
  const out = await client.decide("acme/acme-2", request);
  assert.equal(sent.url, "https://api.acme.test/v1/acme-2");
  assert.equal(sent.headers.authorization, "Bearer k");
  assert.equal(sent.headers["x-gw"], "g");
  assert.deepEqual(out.answers.billing, { kind: "yesno", probability: 0.9 });
  assert.equal(out.probability_source, "provider");
});

test("capabilities are checked before the codec runs", async () => {
  const fetch = async () => assert.fail("must not send");
  const big = { input: "x", questions: Object.fromEntries([1, 2, 3, 4, 5, 6].map((i) => [`q${i}`, { kind: "yesno", instructions: "?" }])) };
  const client = new Client({}, { fetch, wasm, providers: { acme } });
  await assert.rejects(client.decide("acme/m", big), (e) => {
    assert.equal(e.code, "Unsupported");
    assert.deepEqual(e.details, { reason: "too_many_questions", provider: "acme", limit: 5, actual: 6 });
    return true;
  });
});

test("malformed custom answers are BadResponse", async () => {
  const bad = { ...acmeReply, results: [{ id: "billing", p: 1.7 }, acmeReply.results[1]] };
  const client = new Client({}, { fetch: respond(200, bad), wasm, providers: { acme } });
  await assert.rejects(client.decide("acme/m", request), (e) => {
    assert.equal(e.code, "BadResponse");
    assert.equal(e.details.reason, "invalid_number");
    assert.equal(e.details.provider, "acme");
    return true;
  });
});

test("HTTP errors are classified like built-ins", async () => {
  const client = new Client({}, { fetch: respond(429, {}, { "retry-after": "3" }), wasm, providers: { acme } });
  await assert.rejects(client.decide("acme/m", request), { code: "RateLimited", retryable: true, retryAfterMs: 3000 });
});

test("exceptions from user code become ProviderError without their message", async () => {
  const leaky = { ...acme, parse() { throw new Error("body was: secret-token-123"); } };
  const client = new Client({}, { fetch: respond(200, acmeReply), wasm, providers: { acme: leaky } });
  await assert.rejects(client.decide("acme/m", request), (e) => {
    assert.ok(e instanceof LetsDecideError);
    assert.equal(e.code, "ProviderError");
    assert.equal(e.provider, "acme");
    assert.ok(!e.message.includes("secret"));
    assert.ok(e.original.message.includes("secret")); // kept for the user's own debugging
    assert.ok(!inspect(e).includes("secret")); // but loggers don't print it
    return true;
  });
});

test("names: built-ins and duplicates are refused unless replaced", () => {
  const opts = (o) => ({ wasm, fetch: async () => {}, ...o });
  assert.throws(() => new Client({}, opts({ providers: { openai: acme } })), (e) => e.details.reason === "name_taken");
  assert.throws(() => new Client({}, opts({ providers: { acme }, engines: { acme: mockEngine() } })), (e) => e.details.reason === "name_taken");
  assert.throws(() => new Client({}, opts({ providers: { "Bad/Name": acme } })), { code: "InvalidRequest" });
  assert.throws(() => new Client({}, opts({ providers: { acme: { build() {} } } })), TypeError);
  new Client({}, opts({ providers: { openai: acme }, replace: ["openai"] }));
});

test("replace routes a built-in name to the custom provider", async () => {
  let url;
  const fetch = async (u) => ((url = u), respond(200, acmeReply)());
  const client = new Client({}, { fetch, wasm, providers: { openai: acme }, replace: ["openai"] });
  await client.decide("openai/x", request);
  assert.equal(url, "https://api.acme.test/v1/x");
});

test("engines and the mock engine need no network", async () => {
  const fetch = async () => assert.fail("must not send");
  const client = new Client({}, {
    fetch, wasm,
    engines: { mock: mockEngine({ answers: { billing: { kind: "yesno", probability: 0.8 } } }) },
  });
  const out = await client.decide("mock/any", request);
  assert.deepEqual(out.answers.billing, { kind: "yesno", probability: 0.8 });
  assert.equal(out.answers.team.choice, "billing");
  assert.deepEqual(out.answers.team.probabilities, { billing: 0.5, tech: 0.5 });
});

test("middleware works over custom providers: fall back from an engine to a codec", async () => {
  const down = { async decide() { throw Object.assign(new LetsDecideError({ code: "ProviderUnavailable", message: "x", retryable: true }), {}); } };
  const client = new Client({}, { fetch: respond(200, acmeReply), wasm, providers: { acme }, engines: { local: down } });
  const res = await compose(client, fallback(["acme/acme-2"]))("local/tiny", request);
  assert.equal(res.fellBackFrom, "local/tiny");
  assert.equal(res.model, "acme-2");
});

test("conformance kit: passes good fixtures, reports problems", async () => {
  const good = {
    name: "acme yes/no and choice",
    model: "acme/acme-2",
    config: { api_key: "k" },
    request,
    http: {
      url: "https://api.acme.test/v1/acme-2",
      headers: [["authorization", "Bearer k"], ["content-type", "application/json"]],
      body: { text: "Charged twice.", ask: ["billing", "team"] },
    },
    response: { status: 200, body: acmeReply },
    expected: { ok: { model: "acme-2", input_tokens: null, answers: {
      billing: { kind: "yesno", probability: 0.9 },
      team: { kind: "choice", choice: "billing", confidence: null, probabilities: { billing: 0.7, tech: 0.3 } },
    } } },
  };
  const limited = { name: "429", model: "acme/m", request, response: { status: 429, headers: [["retry-after", "2"]], body: {} }, expected: { error: { code: "RateLimited", retry_after_ms: 2000 } } };
  const wrongUrl = { ...good, name: "wrong url", http: { ...good.http, url: "https://elsewhere" } };
  const report = await checkProvider(acme, [good, limited, wrongUrl]);
  assert.equal(report.passed, 2);
  assert.deepEqual(report.results[2].problems, ["url: got https://api.acme.test/v1/acme-2, want https://elsewhere"]);
});

test("conformance kit runs engines too", async () => {
  const fx = { model: "mock/x", request, expected: { ok: { model: "mock", input_tokens: null, answers: {
    billing: { kind: "yesno", probability: 0.5 },
    team: { kind: "choice", choice: "billing", confidence: 0.5, probabilities: { billing: 0.5, tech: 0.5 } },
  } } } };
  const report = await checkProvider(mockEngine(), [fx]);
  assert.deepEqual(report.results[0].problems, []);
});

test("conformance kit accepts the core's own golden fixtures for a built-in shim", async () => {
  // A codec that delegates to the core proves the fixture format round-trips.
  const dir = new URL("../../../tests/fixtures/openai/", import.meta.url);
  const fixtures = readdirSync(dir).map((f) => ({ name: f, ...JSON.parse(readFileSync(new URL(f, dir))) }));
  const shim = {
    build(model, req, c) {
      const cfg = { api_key: c.apiKey, access_token: c.accessToken, base_url: c.baseUrl, account_id: c.accountId };
      const out = JSON.parse(core.build(`openai/${model}`, JSON.stringify(req), JSON.stringify(cfg)));
      if (out.error) throw new LetsDecideError(out.error);
      return out.ok;
    },
    parse(req, res) {
      const out = JSON.parse(core.parse("openai/x", JSON.stringify(req), JSON.stringify(res)));
      if (out.error) throw new LetsDecideError(out.error);
      return out.ok;
    },
  };
  const report = await checkProvider(shim, fixtures);
  assert.equal(report.failed, 0, JSON.stringify(report.results.filter((r) => !r.ok)));
});
