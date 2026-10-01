import { describe, expect, it } from 'vitest';
import { loadSettings, saveSettings } from '../src/settings';

function memoryStorage(initial: Record<string, string> = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k: string) => data[k] ?? null,
    setItem: (k: string, v: string) => {
      data[k] = v;
    },
  };
}

describe('settings', () => {
  it('round-trips through storage', () => {
    const storage = memoryStorage();
    saveSettings({ proxy: 'https://p.test/?u=', variables: { TOKEN: 't' } }, storage);
    expect(loadSettings(storage)).toEqual({ proxy: 'https://p.test/?u=', variables: { TOKEN: 't' } });
  });

  it('returns defaults for missing, corrupt or mistyped data', () => {
    expect(loadSettings(memoryStorage())).toEqual({ proxy: '', variables: {} });
    expect(loadSettings(memoryStorage({ 'openapi-aggregator.settings': '{oops' }))).toEqual({ proxy: '', variables: {} });
    expect(
      loadSettings(memoryStorage({ 'openapi-aggregator.settings': '{"proxy":1,"variables":{"A":2}}' })),
    ).toEqual({ proxy: '', variables: {} });
  });

  it('survives storage that throws', () => {
    const throwing = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadSettings(throwing)).toEqual({ proxy: '', variables: {} });
    expect(() => saveSettings({ proxy: '', variables: {} }, throwing)).not.toThrow();
  });
});
