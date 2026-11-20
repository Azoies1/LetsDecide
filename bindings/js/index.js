import init, { build, parse } from "./pkg/letsdecide_wasm.js";

export class LetsDecideError extends Error {
  constructor({ code, message, status, retry_after_ms, retryable, details }) {
    super(message);
    this.name = "LetsDecideError";
    this.code = code;
    this.status = status ?? undefined;
    this.retryAfterMs = retry_after_ms ?? undefined;
    this.retryable = retryable;
    // Safe to log: names, ids, counts, MIME types. Never keys or content.
    this.details = details ?? undefined;
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
  /**
   * @param config `{ apiKey, accessToken, baseUrl, accountId, headers, options }`. `accessToken` is an
   *   OAuth token (Clef) or a function returning one (sync or async), called on every
   *   request so you can refresh it.
   * @param opts `{ fetch, wasm }`
   */
  constructor(config = {}, { fetch = globalThis.fetch, wasm } = {}) {
    this.cfg = config;
    this.fetch = fetch;
    this.wasm = wasm;
  }

  /** `model`: "provider/model". `request`: `{ input, questions, media? }`. */
  async decide(model, request, { signal } = {}) {
    await initialize(this.wasm);
    const req = JSON.stringify(request);
    const { accessToken, headers = [] } = this.cfg;
    const config = JSON.stringify({
      api_key: this.cfg.apiKey,
      access_token: typeof accessToken === "function" ? await accessToken() : accessToken,
      base_url: this.cfg.baseUrl,
      account_id: this.cfg.accountId,
      headers: Array.isArray(headers) ? headers : Object.entries(headers),
      options: this.cfg.options ?? {},
    });
    const http = unwrap(build(model, req, config));
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
export * from "./middleware.js";
