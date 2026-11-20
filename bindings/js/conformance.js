// Conformance kit: run a custom provider against fixtures in the same format as
// the core's golden fixtures (`tests/fixtures/**.json`).
//
// Fixture: { name?, model, config?, request, http?, response?, expected }
//   - codecs: `http` is the exact request to build ({ url, headers, body }) and
//     `response` the recorded reply ({ status, headers, body }).
//   - engines: no `http` or `response`; `decide` is called directly.
//   - expected: { ok: DecisionResponse } or { error: { code, retry_after_ms? } }.

import { check, initialize, LetsDecideError } from "./index.js";

// Key order doesn't matter; numbers compare after one trip through the core.
function canon(v) {
  if (Array.isArray(v)) return v.map(canon);
  if (v && typeof v === "object") {
    return Object.fromEntries(Object.keys(v).sort().map((k) => [k, canon(v[k])]));
  }
  return v;
}
const same = (a, b) => JSON.stringify(canon(a)) === JSON.stringify(canon(b));
const headerMap = (h = []) =>
  Object.fromEntries((Array.isArray(h) ? h : Object.entries(h)).map(([k, v]) => [k.toLowerCase(), v]));

function camel(config = {}) {
  return {
    apiKey: config.api_key,
    accessToken: config.access_token,
    baseUrl: config.base_url,
    accountId: config.account_id,
    headers: config.headers ?? [],
    options: config.options ?? {},
  };
}

/**
 * Runs `impl` (a codec `{ build, parse }` or an engine `{ decide }`) against `fixtures`.
 * Resolves to `{ passed, failed, results: [{ name, ok, problems }] }`. Never throws for a
 * failing fixture: problems are listed.
 */
export async function checkProvider(impl, fixtures, { wasm } = {}) {
  await initialize(wasm);
  const results = [];
  for (const [i, fx] of fixtures.entries()) {
    const name = fx.name ?? `fixture ${i}`;
    const problems = [];
    const model = fx.model.slice(fx.model.indexOf("/") + 1);
    let got;
    try {
      if (typeof impl.decide === "function") {
        got = await impl.decide(model, fx.request, {});
      } else {
        const http = await impl.build(model, fx.request, camel(fx.config));
        if (fx.http) {
          if (http.url !== fx.http.url) problems.push(`url: got ${http.url}, want ${fx.http.url}`);
          if (!same(headerMap(http.headers), headerMap(fx.http.headers))) problems.push("headers differ");
          let body = http.body;
          try {
            body = typeof body === "string" ? JSON.parse(body) : body;
          } catch {
            problems.push("body is not JSON");
          }
          if (!same(body, fx.http.body)) problems.push("body differs");
        }
        const r = fx.response;
        const body = typeof r.body === "string" ? r.body : JSON.stringify(r.body);
        got = await impl.parse(fx.request, { status: r.status, headers: r.headers ?? [], body });
      }
      got = await check(fx.request, got);
      if (fx.expected.error) {
        problems.push(`expected error ${fx.expected.error.code}, got a response`);
      } else if (!same(got, await check(fx.request, fx.expected.ok))) {
        problems.push("response differs from expected");
      }
    } catch (e) {
      const want = fx.expected.error;
      if (!(e instanceof LetsDecideError)) {
        problems.push(`threw ${e?.constructor?.name ?? typeof e} instead of LetsDecideError`);
      } else if (!want) {
        problems.push(`unexpected ${e.code}: ${e.message}`);
      } else {
        if (e.code !== want.code) problems.push(`error code: got ${e.code}, want ${want.code}`);
        if (want.retry_after_ms !== undefined && e.retryAfterMs !== want.retry_after_ms) {
          problems.push(`retryAfterMs: got ${e.retryAfterMs}, want ${want.retry_after_ms}`);
        }
      }
    }
    results.push({ name, ok: problems.length === 0, problems });
  }
  const passed = results.filter((r) => r.ok).length;
  return { passed, failed: results.length - passed, results };
}
