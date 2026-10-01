import { beforeEach, describe, expect, it, vi } from 'vitest';

const scalar = vi.hoisted(() => ({
  created: [] as string[],
  updates: [] as string[],
  failNext: false,
}));

vi.mock('@scalar/api-reference', () => ({
  createApiReference: (_el: unknown, config: { content: string }) => {
    if (scalar.failNext) {
      scalar.failNext = false;
      throw new Error('offline');
    }
    scalar.created.push(config.content);
    return { updateConfiguration: (c: { content: string }) => scalar.updates.push(c.content) };
  },
}));
vi.mock('@scalar/api-reference/style.css', () => ({}));

const el = {} as HTMLElement;

async function freshModule() {
  vi.resetModules();
  return import('../src/ui/reference');
}

beforeEach(() => {
  scalar.created = [];
  scalar.updates = [];
  scalar.failNext = false;
});

describe('showReference', () => {
  it('shows the latest content when calls arrive during the first load', async () => {
    const { showReference } = await freshModule();
    await Promise.all([showReference(el, 'old'), showReference(el, 'new')]);
    const shown = scalar.updates.at(-1) ?? scalar.created.at(-1);
    expect(shown).toBe('new');
    expect(scalar.created).toHaveLength(1);
  });

  it('retries after a failed load', async () => {
    const { showReference } = await freshModule();
    scalar.failNext = true;
    await expect(showReference(el, 'a')).rejects.toThrow('offline');
    await showReference(el, 'b');
    expect(scalar.created).toEqual(['b']);
  });
});
