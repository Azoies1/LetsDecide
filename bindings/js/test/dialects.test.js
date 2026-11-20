import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Client, LetsDecideError, dialect, initialize, resolveDialect } from "../index.js";

const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
await initialize(wasm);

const request = { input: "x", questions: { q: { kind: "yesno", instructions: "?" } } };
const recorder = (status, body) => {
  const sent = [];
  const fetch = async (url, init) => {
    sent.push({ url, headers: init.headers, body: JSON.parse(init.body) });
    return new Response(JSON.stringify(body), { status });
  };
  return { sent, fetch };
};

test("a dialect is a provider described as data", async () => {
  const { sent, fetch } = recorder(200, { answers: { q: { noul: 0.75 } } });
  const client = new Client(
    { byProvider: { acme: { apiKey: "k" } }, headers: { "x-trace": "t" } },
    { fetch, wasm, dialects: { acme: { extends: "jev", base_url: "https://api.acme.test", path: "/v1/decisions" } } },
  );
  const out = await client.decide("acme/acme-1", request);
  assert.equal(sent[0].url, "https://api.acme.test/v1/decisions");
  assert.equal(sent[0].headers.authorization, "Bearer k");
  assert.equal(sent[0].headers["x-trace"], "t");
  assert.deepEqual(sent[0].body.questions, { q: { type: "noul", instructions: "?" } });
  assert.deepEqual(out.answers.q, { kind: "yesno", probability: 0.75 });
});

test("patching a built-in after an API change needs only data", async () => {
  const { sent, fetch } = recorder(200, { answers: { q: { noul: 0.5 } } });
  const client = new Client(
    { byProvider: { gemini: { apiKey: "k" } } },
    { fetch, wasm, dialects: { gemini: { extends: "gemini", path: "/v1/decisions" } }, replace: ["gemini"] },
  );
  await client.decide("gemini/gemini-decision-2", request);
  assert.equal(sent[0].url, "https://generativelanguage.googleapis.com/v1/decisions");
  assert.equal(sent[0].headers["x-goog-api-key"], "k");
});

test("dialects get the same checks and error details", async () => {
  const { fetch } = recorder(200, { answers: { q: { noul: 1.5 } } });
  const client = new Client({}, {
    fetch, wasm,
    dialects: { tiny: { extends: "systemone", base_url: "https://h.test", capabilities: { max_questions: 1 } } },
  });
  await assert.rejects(client.decide("tiny/m", request), (e) => e.code === "BadResponse" && e.details.provider === "tiny");
  const two = { input: "x", questions: { a: request.questions.q, b: request.questions.q } };
  await assert.rejects(client.decide("tiny/m", two), (e) => e.details.reason === "too_many_questions" && e.details.provider === "tiny");
  assert.equal((await client.capabilities("tiny/m")).max_questions, 1);
});

test("a bad dialect fails with the field named; built-in names need replace", async () => {
  await assert.rejects(resolveDialect({ extends: "jev", pathh: "/x" }), (e) => {
    assert.ok(e instanceof LetsDecideError);
    assert.match(e.message, /unknown field `pathh`/);
    return true;
  });
  const client = new Client({}, { fetch: async () => {}, wasm, dialects: { bad: { extends: "nope" } } });
  await assert.rejects(client.decide("bad/m", request), (e) => e.details.reason === "invalid_config" && e.details.provider === "bad");
  assert.throws(() => new Client({}, { wasm, dialects: { jev: { extends: "jev" } } }), (e) => e.details.reason === "name_taken");
});

test("built-in dialects can be read and extended", async () => {
  const gemini = await dialect("gemini");
  assert.equal(gemini.auth.header, "x-goog-api-key");
  assert.equal(gemini.family, "systemone");
  const full = await resolveDialect({ extends: "openai", base_url: "https://az.test/openai" });
  assert.equal(full.path, "/v1/decisions");
  assert.equal(full.base_url, "https://az.test/openai");
});
