export type Json = string | number | boolean | null | Json[] | { [k: string]: Json };

export interface Media {
  /** `image/*`, `audio/*`, `video/*` or a document type; non-images: Gemini only. */
  mime_type: string;
  /** Base64, no `data:` prefix. Give `data` or `uri`, not both. */
  data?: string;
  /** Gemini only: `https://` URL or Files API URI (not YouTube or `gs://`). */
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
  cached_tokens?: number;
  thought_tokens?: number;
  /** Present when every answer is `refused` and the provider gave a reason. */
  block_reason?: string;
}

export interface ClientConfig {
  /** Static key. Required for `gemini/*`. */
  apiKey?: string;
  /** OAuth access token for `clef/*`, or a function returning a fresh one (called per request). Wins over `apiKey`. */
  accessToken?: string | (() => string | Promise<string>);
  /** Required for `systemone/*`. */
  baseUrl?: string;
  /** Required for `clef/*`. */
  accountId?: string;
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
  | "BadResponse";

export class LetsDecideError extends Error {
  code: ErrorCode;
  status?: number;
  retryAfterMs?: number;
  retryable: boolean;
}

export function initialize(wasm?: BufferSource | WebAssembly.Module): Promise<unknown>;

export class Client {
  constructor(config?: ClientConfig, options?: ClientOptions);
  /** `model` is `"provider/model"`, e.g. `"clef/clef-flash"`. */
  decide(model: string, request: DecisionRequest, options?: { signal?: AbortSignal }): Promise<DecisionResponse>;
}
export * from "./middleware";
