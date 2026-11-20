import init, * as core from "./pkg/letsdecide_wasm.js";

const { build, parse } = core;

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

/** Built-in provider names; custom providers can't take them without `replace`. */
export const BUILTIN = ["jev", "clef", "gemini", "openai", "systemone"];

const NAME = /^[a-z0-9_-]{1,64}$/;
const HEADER_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;

function nameTaken(name) {
  return new LetsDecideError({
    code: "InvalidRequest",
    message: `provider \`${name}\` is already registered; list it in \`replace\` to override it`,
    retryable: false,
    details: { reason: "name_taken", provider: name },
  });
}

/** Tags core errors with the provider; wraps anything else as `ProviderError`. */
function fromUserCode(name, e) {
  if (e instanceof LetsDecideError) {
    if (e.details && e.details.provider === undefined) e.details.provider = name;
    return e;
  }
  if (e?.name === "AbortError") return e; // cancellation stays cancellation
  // Never serialise the user's message: it may hold bodies or keys.
  const err = new LetsDecideError({
    code: "ProviderError",
    message: `custom provider \`${name}\` threw ${e?.constructor?.name ?? typeof e}`,
    retryable: false,
  });
  err.provider = name;
  // Non-enumerable and not `cause`, so loggers and `util.inspect` don't print it.
  Object.defineProperty(err, "original", { value: e, enumerable: false });
  return err;
}

/** `config.headers` (object or pairs) as pairs, after the codec's own; same name replaces. */
function withHeaders(headers, extra) {
  const out = (headers ?? []).filter(([k]) => !extra.some(([e]) => e.toLowerCase() === k.toLowerCase()));
  for (const [k, v] of extra) {
    if (!HEADER_NAME.test(k) || /[\r\n\0]/.test(v)) {
      throw new LetsDecideError({
        code: "InvalidRequest",
        message: "a header in `headers` is not valid",
        retryable: false,
        details: { reason: "invalid_config", field: "headers" },
      });
    }
    out.push([k, v]);
  }
  return out;
}

const pairs = (h = []) => (Array.isArray(h) ? h : Object.entries(h ?? {}));

/** Client-wide settings. Everything else is per provider. */
const SHARED = ["byProvider", "headers"];
const PER_PROVIDER = ["apiKey", "accessToken", "baseUrl", "accountId", "headers", "options"];

/** How deep engines may call back into the client (a panel of panels...). */
export const MAX_DEPTH = 4;
const DEPTH = Symbol("letsdecide.depth");

// Config values may be functions (sync or async), resolved on every call.
const resolve = async (v) => (typeof v === "function" ? await v() : v);

/** Validates `request` (and checks it against `capabilities`, if given). Throws `LetsDecideError`. */
export async function validate(request, capabilities = null, { wasm } = {}) {
  await initialize(wasm);
  unwrap(core.validate(JSON.stringify(request), JSON.stringify(capabilities)));
}

/** Checks a canonical `response` against `request`; returns it with defaults filled. */
export async function check(request, response, { wasm } = {}) {
  await initialize(wasm);
  return unwrap(core.check(JSON.stringify(request), JSON.stringify(response)));
}

/** The error a built-in adapter would raise for this non-2xx reply (`{status, headers, body}`). */
export async function classifyHttp(response, { wasm } = {}) {
  await initialize(wasm);
  const out = unwrap(core.classify_http(JSON.stringify({
    status: response.status,
    headers: pairs(response.headers),
    body: response.body ?? "",
  })));
  return new LetsDecideError(out);
}

/** Capabilities of a built-in `"provider/model"`. */
export async function capabilities(model, { wasm } = {}) {
  await initialize(wasm);
  return unwrap(core.capabilities(model));
}

export class Client {
  /**
   * @param config `{ byProvider, headers }`. `byProvider`: settings per provider prefix,
   *   `{ gemini: { apiKey }, clef: { accessToken, accountId }, acme: { options } }`; a provider
   *   only ever sees its own entry. Any value may be a function (sync or async), called on every
   *   request, e.g. to refresh an OAuth token. `headers`: sent to every provider (tracing,
   *   gateways); never put secrets there.
   * @param opts `{ fetch, wasm, providers, engines, dialects, replace }`. `providers`: custom HTTP
   *   codecs `{ build, parse, capabilities? }` by name. `engines`: custom `{ decide,
   *   capabilities? }` by name (local models, mocks). `dialects`: providers described as data,
   *   e.g. `{ acme: { extends: "jev", base_url, path } }`. `replace`: built-in names they may
   *   override (e.g. a patched `gemini` dialect).
   */
  constructor(config = {}, { fetch = globalThis.fetch, wasm, providers = {}, engines = {}, dialects = {}, replace = [] } = {}) {
    for (const k of Object.keys(config)) {
      if (!SHARED.includes(k)) {
        const hint = PER_PROVIDER.includes(k) ? `set it per provider: byProvider.<name>.${k}` : "unknown setting";
        throw new LetsDecideError({
          code: "InvalidRequest",
          message: `config \`${k}\`: ${hint}`,
          retryable: false,
          details: { reason: "invalid_config", field: k },
        });
      }
    }
    for (const [name, own] of Object.entries(config.byProvider ?? {})) {
      const bad = Object.keys(own ?? {}).find((k) => !PER_PROVIDER.includes(k));
      if (bad) {
        throw new LetsDecideError({
          code: "InvalidRequest",
          message: `byProvider.${name}: unknown setting \`${bad}\``,
          retryable: false,
          details: { reason: "invalid_config", field: "byProvider", provider: name },
        });
      }
    }
    this.cfg = config;
    this.fetch = fetch;
    this.wasm = wasm;
    this.custom = new Map();
    const fromDialects = Object.fromEntries(Object.entries(dialects).map(([n, spec]) => [n, dialectCodec(spec)]));
    for (const [kind, table] of [["codec", providers], ["engine", engines], ["codec", fromDialects]]) {
      for (const [name, impl] of Object.entries(table)) {
        if (!NAME.test(name)) {
          throw new LetsDecideError({
            code: "InvalidRequest",
            message: "provider names must match `[a-z0-9_-]{1,64}`",
            retryable: false,
            details: { reason: "invalid_config", field: "name" },
          });
        }
        if (this.custom.has(name) || (BUILTIN.includes(name) && !replace.includes(name))) throw nameTaken(name);
        const needs = kind === "codec" ? ["build", "parse"] : ["decide"];
        for (const fn of needs) {
          if (typeof impl?.[fn] !== "function") throw new TypeError(`${kind} \`${name}\` needs a \`${fn}\` function`);
        }
        this.custom.set(name, { kind, impl });
      }
    }
  }

  /** Settings for one provider: its own `byProvider` entry plus the shared headers. */
  async #config(name) {
    const own = this.cfg.byProvider?.[name] ?? {};
    const shared = pairs(await resolve(this.cfg.headers));
    const mine = pairs(await resolve(own.headers));
    return {
      apiKey: await resolve(own.apiKey),
      accessToken: await resolve(own.accessToken),
      baseUrl: await resolve(own.baseUrl),
      accountId: await resolve(own.accountId),
      // The provider's own headers win over shared ones with the same name.
      headers: [...shared.filter(([k]) => !mine.some(([m]) => m.toLowerCase() === k.toLowerCase())), ...mine],
      options: (await resolve(own.options)) ?? {},
    };
  }

  /** Capabilities of any `"provider/model"`: built-in via the core, custom from the provider. */
  async capabilities(model) {
    await initialize(this.wasm);
    const name = model.slice(0, model.indexOf("/"));
    const custom = name && this.custom.get(name);
    if (custom) return custom.impl.capabilities ?? {};
    return unwrap(core.capabilities(model));
  }

  async #send(http, signal) {
    const res = await this.fetch(http.url, {
      method: http.method,
      headers: Object.fromEntries(http.headers),
      body: http.body,
      signal,
    });
    return { status: res.status, headers: [...res.headers], body: await res.text() };
  }

  /** `model`: "provider/model". `request`: `{ input, questions, media? }`. */
  async decide(model, request, { signal, [DEPTH]: depth = 0 } = {}) {
    await initialize(this.wasm);
    const name = model.slice(0, model.indexOf("/"));
    const custom = name && this.custom.get(name);
    if (custom) return this.#decideCustom(name, model.slice(name.length + 1), custom, request, signal, depth);
    const req = JSON.stringify(request);
    const cfg = await this.#config(name);
    const config = JSON.stringify(coreConfig(cfg));
    const http = unwrap(build(model, req, config));
    const response = await this.#send(http, signal);
    return unwrap(parse(model, req, JSON.stringify(response)));
  }

  // Same guarantees as built-ins: validate first, check the answer after.
  async #decideCustom(name, model, { kind, impl }, request, signal, depth) {
    const tag = (e) => fromUserCode(name, e);
    if (!model) throw new LetsDecideError({ code: "InvalidRequest", message: "model must look like `provider/model`", retryable: false });
    try {
      unwrap(core.validate(JSON.stringify(request), JSON.stringify(impl.capabilities ?? null)));
    } catch (e) {
      throw tag(e);
    }
    let out;
    if (kind === "engine") {
      try {
        if (depth >= MAX_DEPTH) {
          throw new LetsDecideError({
            code: "InvalidRequest",
            message: `engines nested more than ${MAX_DEPTH} deep (does an engine call itself?)`,
            retryable: false,
            details: { reason: "invalid_config", field: "engines" },
          });
        }
        // Engines may call other models through the client (e.g. a panel's members).
        const decide = (m, r, o = {}) => this.decide(m, r, { signal, ...o, [DEPTH]: depth + 1 });
        out = await impl.decide(model, request, { signal, decide });
      } catch (e) {
        throw tag(e);
      }
    } else {
      const cfg = await this.#config(name);
      let http;
      try {
        http = await impl.build(model, request, cfg);
        http = { method: http.method ?? "POST", url: http.url, headers: withHeaders(pairs(http.headers), cfg.headers), body: http.body };
      } catch (e) {
        throw tag(e);
      }
      const response = await this.#send(http, signal);
      try {
        out = await impl.parse(request, response);
      } catch (e) {
        throw tag(e);
      }
    }
    try {
      // Checked fields win; extra fields an engine adds are kept, unchecked.
      return { ...out, ...unwrap(core.check(JSON.stringify(request), JSON.stringify(out))) };
    } catch (e) {
      throw tag(e);
    }
  }
}

/** Config as the core takes it (snake_case). */
const coreConfig = (c) => ({
  api_key: c.apiKey,
  access_token: c.accessToken,
  base_url: c.baseUrl,
  account_id: c.accountId,
  headers: c.headers,
  options: c.options,
});

/**
 * A dialect run by the core, as a codec. Resolved (and validated) once, on first use,
 * because the WASM core loads asynchronously; call `resolveDialect` to check one up front.
 */
function dialectCodec(spec) {
  let resolved;
  const get = () => (resolved ??= JSON.stringify(unwrap(core.resolve_dialect(JSON.stringify(spec)))));
  return {
    get capabilities() {
      return JSON.parse(get()).capabilities;
    },
    build(model, request, config) {
      return unwrap(core.build_dialect(get(), model, JSON.stringify(request), JSON.stringify(coreConfig(config))));
    },
    parse(request, response) {
      return unwrap(core.parse_dialect(get(), JSON.stringify(request), JSON.stringify(response)));
    },
  };
}

/** A built-in dialect (`jev`, `clef`, `gemini`, `openai`, `systemone`) as JSON, to read or extend. */
export async function dialect(name, { wasm } = {}) {
  await initialize(wasm);
  return unwrap(core.dialect(name));
}

/** Validates a dialect and expands `extends`; throws `LetsDecideError` on a bad one. */
export async function resolveDialect(spec, { wasm } = {}) {
  await initialize(wasm);
  return unwrap(core.resolve_dialect(JSON.stringify(spec)));
}

/**
 * A deterministic engine for tests: no network, no keys. Answers each question with
 * `answers[id]` if given, else a neutral answer (yes/no 0.5, first option, middle level).
 */
export function mockEngine({ answers = {}, model = "mock", probabilitySource = "provider" } = {}) {
  return {
    capabilities: { media: ["image/*", "audio/*", "video/*", "application/pdf"] },
    async decide(_model, request) {
      const out = {};
      for (const [id, q] of Object.entries(request.questions)) {
        if (answers[id]) {
          out[id] = answers[id];
        } else if (q.kind === "yesno") {
          out[id] = { kind: "yesno", probability: 0.5 };
        } else {
          const ids = q.kind === "choice" ? Object.keys(q.options).sort() : q.levels.map((l) => l.id);
          const p = 1 / ids.length;
          const probabilities = Object.fromEntries(ids.map((i) => [i, p]));
          out[id] = q.kind === "choice"
            ? { kind: "choice", choice: ids[0], confidence: p, probabilities }
            : { kind: "score", value: (ids.length - 1) / 2, confidence: p, probabilities };
        }
      }
      return { model, answers: out, input_tokens: null, probability_source: probabilitySource };
    },
  };
}

export * from "./middleware.js";
