import type { Codec, DecisionRequest, DecisionResponse, Engine, Json } from "./index.js";

export interface Fixture {
  name?: string;
  model: string;
  config?: Record<string, Json>;
  request: DecisionRequest;
  /** Codecs: the exact request to build. */
  http?: { url: string; headers: [string, string][] | Record<string, string>; body: Json };
  /** Codecs: the recorded reply. */
  response?: { status: number; headers?: [string, string][]; body: Json };
  expected: { ok: DecisionResponse } | { error: { code: string; retry_after_ms?: number } };
}

export interface Report {
  passed: number;
  failed: number;
  results: { name: string; ok: boolean; problems: string[] }[];
}

export function checkProvider(impl: Codec | Engine, fixtures: Fixture[], options?: { wasm?: BufferSource | WebAssembly.Module }): Promise<Report>;
