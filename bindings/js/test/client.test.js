import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Client, LetsDecideError } from "../index.js";

const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
const request = {
  input: "Charged twice.",
  questions: {
    billing: { kind: "yesno", instructions: "Is this about billing?" },
    team: { kind: "choice", instructions: "Which team?", options: { billing: "Payments", tech: "Bugs" } },
  },
};
const reply = (status, body, headers = {}) => async () =>
  new Response(JSON.stringify(body), { status, headers });

test("decide builds the request, sends it and parses the reply", async () => {
  let sent;
  const fetch = async (url, init) => {
    sent = { url, ...init, body: JSON.parse(init.body) };
    return reply(200, {
      answers: {
        billing: { type: "noul", noul: 0.9 },
        team: { type: "choice", choice: "billing", confidence: 0.8, probabilities: { billing: 0.8, tech: 0.2 } },
      },
      usage: { total_input_tokens: 7 },
    })();
  };
  const client = new Client({ apiKey: "k" }, { fetch, wasm });
  const out = await client.decide("gemini/gemini-decision-1", request);

  assert.equal(sent.url, "https://generativelanguage.googleapis.com/v1beta/decisions");
  assert.equal(sent.headers["x-goog-api-key"], "k");
  assert.equal(sent.body.model, "gemini-decision-1");
  assert.deepEqual(out.answers.billing, { kind: "yesno", probability: 0.9 });
  assert.equal(out.answers.team.choice, "billing");
  assert.equal(out.input_tokens, 7);
});

test("switching provider is one string", async () => {
  const urls = [];
  const fetch = async (url) => {
    urls.push(url);
    return reply(401, {})();
  };
  const client = new Client({ apiKey: "k", accountId: "a" }, { fetch, wasm });
  for (const m of ["jev/jev-latest", "openai/gpt-6-luna", "clef/clef-flash"]) {
    await assert.rejects(client.decide(m, request), { code: "Auth" });
  }
  assert.deepEqual(urls, [
    "https://api.typesafe.ai/v1/systemone",
    "https://api.openai.com/v1/decisions",
    "https://api.cloudflare.com/client/v4/accounts/a/ai/run/@cf/cloudflare/clef-flash",
  ]);
});

test("errors are typed and carry retry info", async () => {
  const fetch = reply(429, {}, { "retry-after": "2" });
  const client = new Client({ apiKey: "k" }, { fetch, wasm });
  await assert.rejects(client.decide("jev/jev-latest", request), (e) => {
    assert.ok(e instanceof LetsDecideError);
    assert.equal(e.code, "RateLimited");
    assert.equal(e.retryable, true);
    assert.equal(e.retryAfterMs, 2000);
    return true;
  });
});

test("invalid requests fail before any network call", async () => {
  const fetch = async () => assert.fail("must not fetch");
  const client = new Client({}, { fetch, wasm });
  const bad = { input: "x", questions: { "Bad-Id": { kind: "yesno", instructions: "?" } } };
  await assert.rejects(client.decide("jev/jev-latest", bad), { code: "InvalidRequest" });
});

test("clef uses an OAuth token, refreshed per request via a function", async () => {
  const seen = [];
  const fetch = async (_url, init) => {
    seen.push(init.headers.authorization);
    return reply(401, {})();
  };
  let n = 0;
  const client = new Client({ accountId: "a", accessToken: async () => `tok${++n}` }, { fetch, wasm });
  for (let i = 0; i < 2; i++) await assert.rejects(client.decide("clef/clef-flash", request), { code: "Auth" });
  assert.deepEqual(seen, ["Bearer tok1", "Bearer tok2"]);
});

test("gemini and clef fail fast without credentials", async () => {
  const fetch = async () => assert.fail("must not send");
  const client = new Client({ accountId: "a" }, { fetch, wasm });
  for (const m of ["gemini/gemini-decision-1", "clef/clef-flash"]) {
    await assert.rejects(client.decide(m, request), { code: "InvalidRequest" });
  }
});
