import type { Answer, DecisionRequest, DecisionResponse, Client, LetsDecideError } from "./index";

export type DecideOptions = { signal?: AbortSignal };
export type Decide = (model: string, request: DecisionRequest, options?: DecideOptions) => Promise<DecisionResponse>;
export type Middleware = (next: Decide) => Decide;
export type Calibrator = (p: number) => number;

declare module "./index" {
  interface DecisionResponse {
    /** Calls made, including failed ones. Set by `retry` and `fallback`. */
    attempts?: number;
    /** The requested model, when a fallback model answered. */
    fellBackFrom?: string;
    /** The requested model, when a hedged call answered. */
    hedgedFrom?: string;
    /** Number of requests `split` sent. */
    parts?: number;
    /** Set by `split` when parts were answered by different models. */
    models?: string[];
  }
}

export type Calibrated = { probability: number } | { probabilities: Record<string, number>; confidence: number };
export type CalibratedAnswer = Answer & { calibrated?: Calibrated };

export interface Comparison {
  /** Share of comparable questions that agree; `null` when none are comparable. */
  agreement: number | null;
  questions: Record<string, { agree: boolean; delta?: number }>;
}

export interface ShadowResult {
  model: string;
  shadowModel: string;
  request: DecisionRequest;
  primary: DecisionResponse;
  shadow?: DecisionResponse;
  error?: LetsDecideError;
  comparison?: Comparison;
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
/** Starts another call after `delayMs` (default 1000) or a recoverable failure; first answer wins. */
export function hedge(options?: { delayMs?: number; models?: string[] }): Middleware;
/** Splits over-limit requests by question and merges the answers. */
export function split(options?: { maxQuestions?: number | ((model: string) => number | undefined); concurrency?: number }): Middleware;
/** Mirrors traffic to `model` in the background and reports both answers. */
export function shadow(options: {
  model: string;
  onResult: (result: ShadowResult) => void;
  /** Share of requests mirrored, 0..1. Default 1. */
  sample?: number;
  random?: () => number;
}): Middleware;
export function compare(a: DecisionResponse, b: DecisionResponse): Comparison;
/** Adds `calibrated` to each answer; raw values are unchanged. */
export function calibrate(calibrators: Calibrator | Record<string, Calibrator>): Middleware;
/** Isotonic fit from `[predicted, outcome]` pairs (outcome 0 or 1). */
export function fitIsotonic(samples: [number, number][]): Calibrator;
export function brierScore(samples: [number, number][]): number | null;
export function observe(hooks?: {
  onStart?: (e: { model: string; request: DecisionRequest }) => void;
  onEnd?: (e: { model: string; request: DecisionRequest; response: DecisionResponse; durationMs: number }) => void;
  onError?: (e: { model: string; request: DecisionRequest; error: unknown; durationMs: number }) => void;
  now?: () => number;
}): Middleware;

/** One model in a `fanOut`. `questions`: send it only these question ids (default: all). */
export type FanOutMember = string | { model: string; enabled?: boolean; questions?: string[] };

/** What happened to one member. `skipped`: it can't take the request (`Unsupported`, no call made). */
export type FanOutResult = (
  | { response: DecisionResponse }
  | { error: LetsDecideError | Error }
  | { skipped: true }
  | { timedOut: true }
) & { ms: number };

/**
 * Asks several models the same request at once and returns every result, keyed by model.
 * Nothing is merged. Bad member lists throw `RangeError` before any call.
 */
export function fanOut(
  decide: Decide,
  request: DecisionRequest,
  members: FanOutMember[],
  options?: { timeoutMs?: number; signal?: AbortSignal },
): Promise<Record<string, FanOutResult>>;
