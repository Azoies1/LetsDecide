import type { DecisionRequest, DecisionResponse, Client } from "./index";

export type DecideOptions = { signal?: AbortSignal };
export type Decide = (model: string, request: DecisionRequest, options?: DecideOptions) => Promise<DecisionResponse>;
export type Middleware = (next: Decide) => Decide;

declare module "./index" {
  interface DecisionResponse {
    /** Calls made, including failed ones. Set by `retry` and `fallback`. */
    attempts?: number;
    /** The requested model, when a fallback model answered. */
    fellBackFrom?: string;
  }
}

export function compose(client: Pick<Client, "decide">, ...middleware: Middleware[]): Decide;
export function retry(options?: {
  maxAttempts?: number;
  baseMs?: number;
  /** Longest wait, including a server `Retry-After`. Default 30000. */
  maxMs?: number;
  /** Injectable for tests. Should reject when `signal` aborts. */
  sleep?: (ms: number, signal?: AbortSignal) => Promise<void>;
}): Middleware;
/** Tries `model`, then each of `models`, on retryable or `Unsupported` errors. */
export function fallback(models: string[]): Middleware;
