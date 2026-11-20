import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Client } from "../index.js";
import { compose, retry, fallback } from "../middleware.js";

const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
const request = { input: "x", questions: { q: { kind: "yesno", instructions: "?" } } };
const ok = { answers: { q: { type: "noul", noul: 0.7 } }, usage: { total_input_tokens: 1 } };

// Replies from a script of [status, body, headers], recording each call's URL.
function setup(script) {
  const urls = [];
  const fetch = async (url) => {
    urls.push(url);
    const [status, body = {}, headers = {}] = script.shift();
    return new Response(JSON.stringify(body), { status, headers });
  };
  return { urls, client: new Client({ byProvider: { jev: { apiKey: "k" }, openai: { apiKey: "k" }, gemini: { apiKey: "k" }, clef: { apiKey: "k", accountId: "a" } } }, { fetch, wasm }) };
}
const sleeps = [];
const sleep = async (ms) => void sleeps.push(ms);

test("retry then succeed, with attempts", async () => {
  sleeps.length = 0;
  const { client, urls } = setup([[503], [503], [200, ok]]);
  const res = await compose(client, retry({ sleep }))("jev/jev-latest", request);
  assert.equal(res.attempts, 3);
  assert.equal(res.fellBackFrom, undefined);
  assert.equal(urls.length, 3);
  assert.equal(sleeps.length, 2);
  assert.ok(sleeps[0] >= 100 && sleeps[0] <= 200 && sleeps[1] >= 200 && sleeps[1] <= 400);
});

test("retry-after is honoured", async () => {
  sleeps.length = 0;
  const { client } = setup([[429, {}, { "retry-after": "2" }], [200, ok]]);
  await compose(client, retry({ sleep }))("jev/jev-latest", request);
  assert.deepEqual(sleeps, [2000]);
});

test("non-retryable errors are not retried; exhausted retries throw", async () => {
  const a = setup([[401]]);
  await assert.rejects(compose(a.client, retry({ sleep }))("jev/jev-latest", request), { code: "Auth", attempts: 1 });
  const b = setup([[503], [503]]);
  await assert.rejects(compose(b.client, retry({ sleep, maxAttempts: 2 }))("jev/jev-latest", request), { attempts: 2 });
});

test("fallback on 5xx and on Unsupported", async () => {
  const { client, urls } = setup([[503], [200, ok]]);
  const res = await compose(client, fallback(["gemini/gemini-decision-1"]))("jev/jev-latest", request);
  assert.equal(res.fellBackFrom, "jev/jev-latest");
  assert.equal(res.attempts, 2);
  assert.match(urls[1], /googleapis/);

  // An unknown provider is Unsupported, raised before any HTTP call.
  const u = setup([[200, ok]]);
  const out = await compose(u.client, fallback(["gemini/gemini-decision-1"]))("nope/model", request);
  assert.equal(out.fellBackFrom, "nope/model");
});

test("fallback throws the last error when all fail, and skips non-retryable", async () => {
  const a = setup([[503], [503]]);
  await assert.rejects(compose(a.client, fallback(["openai/gpt-6-luna"]))("jev/jev-latest", request), { attempts: 2 });
  const b = setup([[401]]);
  await assert.rejects(compose(b.client, fallback(["openai/gpt-6-luna"]))("jev/jev-latest", request), { code: "Auth" });
  assert.equal(b.urls.length, 1);
});

test("abort signal stops retries", async () => {
  const { client, urls } = setup([[503], [200, ok]]);
  const ac = new AbortController();
  const abortingSleep = async () => ac.abort(new Error("stop"));
  await assert.rejects(
    compose(client, retry({ sleep: abortingSleep }))("jev/jev-latest", request, { signal: ac.signal }),
    /stop/,
  );
  assert.equal(urls.length, 1);
});
