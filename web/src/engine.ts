import init, {
  aggregate as wasmAggregate,
  initSync,
  parseConfig as wasmParseConfig,
  pendingRefs as wasmPendingRefs,
} from './wasm/openapi_aggregator_wasm.js';
import type { AggregateOutput, ConfigSummary, PendingRefs } from './types';

export async function initEngine(): Promise<void> {
  await init();
}

export function initEngineSync(bytes: BufferSource): void {
  initSync({ module: bytes });
}

/**
 * Parse and validate `openapi-aggregator.yaml`. Glob sources (`path: specs/*.yaml`) are expanded
 * against `files` (the workspace file names). Throws `Error` with the config error.
 */
export function parseConfig(yaml: string, files: string[] = []): ConfigSummary {
  return wasmParseConfig(yaml, files) as ConfigSummary;
}

/**
 * Which external documents each source still needs. `docs` maps a location (as returned in a
 * previous result) to its text. Throws on config or parse errors.
 */
export function pendingRefs(
  yaml: string,
  contents: string[],
  docs: Record<string, string>,
  files: string[] = [],
): PendingRefs {
  return wasmPendingRefs(yaml, contents, docs, files) as PendingRefs;
}

/**
 * Merge spec texts; `contents[i]` belongs to `parseConfig(yaml).sources[i]`. `docs` holds the
 * external documents the sources reference (location -> text). Throws on failure.
 */
export function aggregate(
  yaml: string,
  contents: string[],
  docs: Record<string, string> = {},
  files: string[] = [],
): AggregateOutput {
  return wasmAggregate(yaml, contents, docs, files) as AggregateOutput;
}
