export type Json = string | number | boolean | null | Json[] | { [k: string]: Json };

export interface Media {
  /** `image/*`, `audio/*`, `video/*` or a document type; non-images: Gemini only. */
  mime_type: string;
  /** Base64, no `data:` prefix. Give `data` or `uri`, not both. */
  data?: string;
  /** Gemini: `https://` URL or Files API URI (not YouTube or `gs://`). OpenAI: public `http(s)://` image URL. */
  uri?: string;
  /** Gemini only, passed through (`resolution`, `fps`, `page_options`, ...). */
  options?: Record<string, Json>;
}

export type Question =
  | {
      kind: "yesno";
      instructions: string;
      /** What yes / no mean. Gemini only. */
      yes_description?: string;
      no_description?: string;
    }
  | { kind: "choice"; instructions: string; options: Record<string, string> }
  | { kind: "score"; instructions: string; levels: { id: string; description: string }[] };

export interface DecisionRequest {
  input: Json;
  media?: Media[];
  /** Ids match `[a-z0-9_]{1,64}`; Gemini also allows `[A-Za-z0-9_-]` keys and wider option ids. */
  questions: Record<string, Question>;
  /** Extra top-level body fields, sent as-is. Keys the adapter sets are rejected. */
  provider_extras?: Record<string, Json>;
}

/** `provider`: sent by the provider. `probabilities`: not sent, read off the chosen option or top level. */
export type ConfidenceSource = "provider" | "probabilities";

export type Answer =
  | { kind: "yesno"; probability: number }
  | { kind: "choice"; choice: string; confidence: number | null; confidence_source?: ConfidenceSource; probabilities: Record<string, number> }
  | { kind: "score"; value: number; confidence: number | null; confidence_source?: ConfidenceSource; probabilities: Record<string, number> }
  | { kind: "refused" };

export interface DecisionResponse {
  model: string | null;
  answers: Record<string, Answer>;
  input_tokens: number | null;
  /** From `spec/prices.json`, when the model's price is known. An estimate. */
  estimated_cost_usd?: number;
  cached_tokens?: number;
  thought_tokens?: number;
  /** Present when every answer is `refused` and the provider gave a reason. */
  block_reason?: string;
  /** Never compare numbers across sources as if on one scale. */
  probability_source: "provider" | "logprob" | "nli" | "vote";
}

type Lazy<T> = T | (() => T | Promise<T>);

/** Per-provider settings, merged over the top-level ones for that provider only. */
export interface ProviderSettings {
  apiKey?: Lazy<string>;
  accessToken?: Lazy<string>;
  baseUrl?: Lazy<string>;
  accountId?: Lazy<string>;
  headers?: Lazy<Record<string, string> | [string, string][]>;
  options?: Lazy<Record<string, Json>>;
}

export interface ClientConfig extends ProviderSettings {
  /**
   * Settings per provider prefix, e.g. `{ jev: { apiKey }, acme: { options } }`, merged over
   * the top-level fields for that provider only. Any value may be a function, called per request.
   * Top-level: `apiKey` (required for `gemini/*`), `accessToken` (OAuth, `clef/*`; wins over
   * `apiKey`), `baseUrl` (required for `systemone/*`), `accountId` (required for `clef/*`),
   * `headers` (replace same-named adapter headers), `options` (for custom providers).
   */
  byProvider?: Record<string, ProviderSettings>;
}

export interface ClientOptions {
  /** Defaults to `globalThis.fetch`. */
  fetch?: typeof fetch;
  /** WASM bytes or module, where the runtime can't `fetch` a file URL (e.g. Node). */
  wasm?: BufferSource | WebAssembly.Module;
  /** Custom HTTP providers by name. */
  providers?: Record<string, Codec>;
  /** Custom engines by name. */
  engines?: Record<string, Engine>;
  /** Built-in names that `providers` or `engines` may override. */
  replace?: string[];
}

export type ErrorCode =
  | "InvalidRequest"
  | "Unsupported"
  | "Auth"
  | "RateLimited"
  | "ProviderUnavailable"
  | "BadResponse"
  | "ProviderError";

export type Reason =
  | "too_many_questions" | "too_many_options" | "too_many_levels" | "too_many_media"
  | "media_not_accepted" | "question_kind" | "unknown_provider" | "no_logprobs"
  | "multi_token_label" | "missing_config" | "invalid_config" | "name_taken"
  | "missing_answer" | "unexpected_answer" | "wrong_answer_kind" | "unknown_id"
  | "invalid_number" | "below_quorum" | "mixed_probability_source";

/** Safe to log by construction: no keys, header values, input or bodies. */
export interface ErrorDetails {
  reason: Reason;
  provider?: string;
  /** Question id. */
  question?: string;
  /** Option or level id. */
  id?: string;
  limit?: number;
  actual?: number;
  mime_type?: string;
  /** Config field name, never its value. */
  field?: string;
}

export class LetsDecideError extends Error {
  code: ErrorCode;
  status?: number;
  retryAfterMs?: number;
  retryable: boolean;
  details?: ErrorDetails;
  /** `ProviderError` only: the custom provider's name. */
  provider?: string;
  /** `ProviderError` only: what user code threw. Non-enumerable, so loggers don't print it. */
  original?: unknown;
}

export function initialize(wasm?: BufferSource | WebAssembly.Module): Promise<unknown>;

export class Client {
  constructor(config?: ClientConfig, options?: ClientOptions);
  /** `model` is `"provider/model"`, e.g. `"clef/clef-flash"`. */
  decide(model: string, request: DecisionRequest, options?: { signal?: AbortSignal }): Promise<DecisionResponse>;
  /** Capabilities of any `"provider/model"`, built-in or custom. */
  capabilities(model: string): Promise<Capabilities>;
}
export * from "./middleware";

export type Capabilities = {
  max_questions?: number | null;
  max_options?: number | null;
  max_levels?: number | null;
  max_media?: number | null;
  /** Accepted MIME types; `type/*` matches a kind. Empty or missing: no media. */
  media?: string[];
  /** `yesno`, `choice`, `score`. Empty or missing: all. */
  question_kinds?: string[];
  /** Allow Gemini-style ids `[A-Za-z0-9_-]{1,128}`. */
  wide_ids?: boolean;
  probabilities?: "provider" | "logprob" | "nli";
  locality?: "cloud" | "network" | "device";
};

/** Config as a custom codec sees it. `accessToken` is already resolved. */
export interface ProviderConfig {
  apiKey?: string;
  accessToken?: string;
  baseUrl?: string;
  accountId?: string;
  headers: [string, string][];
  options: Record<string, Json>;
}

export interface HttpRequestOut {
  method?: string;
  url: string;
  headers?: Record<string, string> | [string, string][];
  body?: string;
}

export interface HttpReply {
  status: number;
  headers: [string, string][];
  body: string;
}

/** A custom HTTP provider. Keep `build` and `parse` pure; the client sends the request. */
export interface Codec {
  build(model: string, request: DecisionRequest, config: ProviderConfig): HttpRequestOut | Promise<HttpRequestOut>;
  /** Throw `LetsDecideError` (e.g. from `classifyHttp`) for failures. */
  parse(request: DecisionRequest, response: HttpReply): DecisionResponse | Promise<DecisionResponse>;
  /** Checked before `build`. Missing: only the request itself is validated. */
  capabilities?: Capabilities;
}

/** A custom provider that isn't one HTTP round trip: local models, mocks, rules. */
export interface Engine {
  /** `options.decide` calls any model through the same client (custom or built-in), e.g. a panel's members. */
  decide(
    model: string,
    request: DecisionRequest,
    options: { signal?: AbortSignal; decide: (model: string, request: DecisionRequest, options?: { signal?: AbortSignal }) => Promise<DecisionResponse> },
  ): Promise<DecisionResponse>;
  capabilities?: Capabilities;
}

export const BUILTIN: readonly string[];
/** How deep engines may call back into the client. */
export const MAX_DEPTH: number;
export function validate(request: DecisionRequest, capabilities?: Capabilities | null, options?: { wasm?: BufferSource | WebAssembly.Module }): Promise<void>;
export function check(request: DecisionRequest, response: unknown, options?: { wasm?: BufferSource | WebAssembly.Module }): Promise<DecisionResponse>;
export function classifyHttp(response: { status: number; headers?: Record<string, string> | [string, string][]; body?: string }, options?: { wasm?: BufferSource | WebAssembly.Module }): Promise<LetsDecideError>;
export function capabilities(model: string, options?: { wasm?: BufferSource | WebAssembly.Module }): Promise<Capabilities>;
export function mockEngine(options?: { answers?: Record<string, Answer>; model?: string; probabilitySource?: "provider" | "logprob" | "nli" }): Engine;

/** One panel: members asked together, combined by `rules`. See docs/concurrent-models.md. */
export interface Panel {
  /** `vote` (default): merge every answer. `race`: first answer wins, the rest are aborted. */
  mode?: "vote" | "race";
  /** `questions`: route only these question ids to the member (vote mode). Default: all. */
  members: { model: string; enabled?: boolean; weight?: number; questions?: string[] }[];
  rules?: {
    /** Minimum members that must answer (default 1). */
    quorum?: number;
    /** Deadline for the whole panel; late members count as `ProviderUnavailable`. */
    timeout_ms?: number;
    /** `vote` (default): weighted majority. `mean`: average, same `probability_source` only. */
    merge?: "vote" | "mean";
  };
}

/** Per member: its response, its error (code and safe details), or `skipped` (can't take the request). */
export type PanelReport = Record<string, ({ response: DecisionResponse } | { error: { code: ErrorCode; details?: ErrorDetails } } | { skipped: true }) & { ms: number }>;

/** A panel's answer: merged (vote) or the winner's (race), plus how each member did. */
export type PanelResponse = DecisionResponse & {
  /** vote: per question, weighted share of voters that agreed with the merged answer. */
  agreement?: Record<string, number>;
  /** vote: questions where the voters were not unanimous. */
  disagreed?: string[];
  panel: PanelReport;
};

/** An engine that asks several models at once. `panels` may be a function, read on every call. */
export function panelEngine(panels: Record<string, Panel> | (() => Record<string, Panel> | Promise<Record<string, Panel>>)): Engine;
