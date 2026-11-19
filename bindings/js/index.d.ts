export type Json = string | number | boolean | null | Json[] | { [k: string]: Json };

export interface Image {
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
  images?: Image[];
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
}

export interface ClientConfig {
  apiKey?: string;
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
