import { readFileSync } from 'node:fs';
import { initEngineSync } from '../src/engine';

let loaded = false;

/** Load the real wasm build (run `npm run wasm` first). */
export function loadEngineForTests(): void {
  if (loaded) return;
  initEngineSync(readFileSync(new URL('../src/wasm/openapi_aggregator_wasm_bg.wasm', import.meta.url)));
  loaded = true;
}
