export type Json = string | number | boolean | null | Json[] | { [k: string]: Json };

export interface Media {
  /** `image/*`, `audio/*`, `video/*` or `application/pdf`; non-images: Gemini only. */
  mime_type: string;
  /** Base64, no `data:` prefix. */
  data: string;
}

export type Question =
  | { kind: "yesno"; instructions: string }
  | { kind: "choice"; instructions: string; options: Record<string, string> }
  | { kind: "score"; instructions: string; levels: { id: string; description: string }[] };

export interface DecisionRequest {
  input: Json;
  media?: Media[];
  /** Ids match `[a-z0-9_]{1,64}`. */
  questions: Record<string, Question>;
}

export type Answer =
  | { kind: "yesno"; probability: number }
  | { kind: "choice"; choice: string; confidence: number | null; probabilities: Record<string, number> }
  | { kind: "score"; value: number; confidence: number | null; probabilities: Record<string, number> }
  | { kind: "refused" };

export interface DecisionResponse {
  model: string | null;
  answers: Record<string, Answer>;
  input_tokens: number | null;
  /** Never compare numbers across sources as if on one scale. */
  probability_source: "provider" | "logprob" | "nli";
}

export interface ClientConfig {
  apiKey?: string;
  /** Required for `systemone/*`. */
  baseUrl?: string;
  /** Required for `clef/*`. */
  accountId?: string;
  /** Extra headers (gateways, tenants); replace same-named adapter headers. */
  headers?: Record<string, string> | [string, string][];
  /** Free-form settings for custom providers; built-ins ignore them. */
  options?: Record<string, Json>;
}

export interface ClientOptions {
  /** Defaults to `globalThis.fetch`. */
  fetch?: typeof fetch;
  /** WASM bytes or module, where the runtime can't `fetch` a file URL (e.g. Node). */
  wasm?: BufferSource | WebAssembly.Module;
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
  | "invalid_number";

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
}

export function initialize(wasm?: BufferSource | WebAssembly.Module): Promise<unknown>;

export class Client {
  constructor(config?: ClientConfig, options?: ClientOptions);
  /** `model` is `"provider/model"`, e.g. `"clef/clef-flash"`. */
  decide(model: string, request: DecisionRequest, options?: { signal?: AbortSignal }): Promise<DecisionResponse>;
}
export * from "./middleware";
