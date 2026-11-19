import init, { build, parse } from "./pkg/letsdecide_wasm.js";

export class LetsDecideError extends Error {
  constructor({ code, message, status, retry_after_ms, retryable }) {
    super(message);
    this.name = "LetsDecideError";
    this.code = code;
    this.status = status ?? undefined;
    this.retryAfterMs = retry_after_ms ?? undefined;
    this.retryable = retryable;
  }
}

// The core returns {ok} or {error}; unwrap or throw.
function unwrap(json) {
  const out = JSON.parse(json);
  if (out.error) throw new LetsDecideError(out.error);
  return out.ok;
}

let ready;
/** Loads the WASM once. Pass bytes or a module where `fetch(url)` isn't available (e.g. Node). */
export function initialize(wasm) {
  return (ready ??= init({ module_or_path: wasm }));
}

export class Client {
  /** @param config `{ apiKey, baseUrl, accountId }` @param opts `{ fetch, wasm }` */
  constructor(config = {}, { fetch = globalThis.fetch, wasm } = {}) {
    this.config = JSON.stringify({
      api_key: config.apiKey,
      base_url: config.baseUrl,
      account_id: config.accountId,
    });
    this.fetch = fetch;
    this.wasm = wasm;
  }

  /** `model`: "provider/model". `request`: `{ input, questions, images? }`. */
  async decide(model, request, { signal } = {}) {
    await initialize(this.wasm);
    const req = JSON.stringify(request);
    const http = unwrap(build(model, req, this.config));
    const res = await this.fetch(http.url, {
      method: http.method,
      headers: Object.fromEntries(http.headers),
      body: http.body,
      signal,
    });
    const response = JSON.stringify({
      status: res.status,
      headers: [...res.headers],
      body: await res.text(),
    });
    return unwrap(parse(model, req, response));
  }
}
