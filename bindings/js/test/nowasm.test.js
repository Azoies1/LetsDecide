import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Client } from "../index.js";

// Its own file: `node --test` runs each file in a process, and the WASM loads once per process.
const request = { input: "x", questions: { q: { kind: "yesno", instructions: "?" } } };
const fetch = async () =>
  new Response(JSON.stringify({ answers: { q: { type: "noul", noul: 0.5 } } }), { status: 200 });
const config = { byProvider: { gemini: { apiKey: "k" } } };

test("in Node without `wasm`, the error says to pass the bytes", async () => {
  const client = new Client(config, { fetch });
  await assert.rejects(client.decide("gemini/gemini-decision-1", request), (e) => {
    assert.match(e.message, /Pass the bytes/);
    assert.ok(e.cause, "the original error is kept");
    return true;
  });
});

test("a failed load is not cached: a later call with the bytes works", async () => {
  const wasm = readFileSync(new URL("../pkg/letsdecide_wasm_bg.wasm", import.meta.url));
  const res = await new Client(config, { fetch, wasm }).decide("gemini/gemini-decision-1", request);
  assert.equal(res.answers.q.probability, 0.5);
});
