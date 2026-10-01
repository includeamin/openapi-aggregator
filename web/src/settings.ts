import type { Settings } from './types';

const KEY = 'openapi-aggregator.settings';

function defaults(): Settings {
  return { proxy: '', variables: {} };
}

function browserStorage(): Storage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

function isStringRecord(value: unknown): value is Record<string, string> {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.values(value).every((v) => typeof v === 'string')
  );
}

/** Proxy and variables live only in this browser; never shared. */
export function loadSettings(storage: Pick<Storage, 'getItem'> | undefined = browserStorage()): Settings {
  try {
    const raw = storage?.getItem(KEY);
    if (!raw) return defaults();
    const parsed: unknown = JSON.parse(raw);
    const { proxy, variables } = (parsed ?? {}) as Record<string, unknown>;
    if (typeof proxy !== 'string' || !isStringRecord(variables)) return defaults();
    return { proxy, variables };
  } catch {
    return defaults();
  }
}

export function saveSettings(
  settings: Settings,
  storage: Pick<Storage, 'setItem'> | undefined = browserStorage(),
): void {
  try {
    storage?.setItem(KEY, JSON.stringify(settings));
  } catch {
    // Storage blocked (private mode, disabled site data): settings just won't persist.
  }
}
