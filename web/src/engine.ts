import init, {
  aggregate as wasmAggregate,
  initSync,
  parseConfig as wasmParseConfig,
} from './wasm/openapi_aggregator_wasm.js';
import type { AggregateOutput, ConfigSummary } from './types';

export async function initEngine(): Promise<void> {
  await init();
}

export function initEngineSync(bytes: BufferSource): void {
  initSync({ module: bytes });
}

/** Parse and validate `openapi-aggregator.yaml`. Throws `Error` with the config error. */
export function parseConfig(yaml: string): ConfigSummary {
  return wasmParseConfig(yaml) as ConfigSummary;
}

/** Merge spec texts; `contents[i]` belongs to `parseConfig(yaml).sources[i]`. Throws on failure. */
export function aggregate(yaml: string, contents: string[]): AggregateOutput {
  return wasmAggregate(yaml, contents) as AggregateOutput;
}
