import { describe, expect, it, vi } from 'vitest';
import { fetchText, memoizeLoader, resolveSources, substituteVars } from '../src/resolve';
import type { Settings, SourceSummary } from '../src/types';

const settings = (over: Partial<Settings> = {}): Settings => ({ proxy: '', variables: {}, ...over });
const fileSource = (path: string, name = 'a'): SourceSummary => ({ name, kind: 'file', path, url: null, headers: {} });
const urlSource = (url: string, name = 'remote'): SourceSummary => ({ name, kind: 'url', path: null, url, headers: { Authorization: 'Bearer ${T}' } });

describe('substituteVars', () => {
  it('replaces placeholders', () => {
    expect(substituteVars('Bearer ${T} ${T}', { T: 'x' })).toBe('Bearer x x');
  });
  it('names a missing variable', () => {
    expect(() => substituteVars('${NOPE}', {})).toThrowError(/'NOPE'/);
  });
  it('does not resolve prototype keys', () => {
    expect(() => substituteVars('${toString}', {})).toThrowError(/'toString'/);
  });
});

describe('fetchText', () => {
  it('applies variables and the proxy prefix', async () => {
    const fetchFn = vi.fn(async () => new Response('spec'));
    const text = await fetchText(
      'https://api.test/${V}.json',
      { Authorization: 'Bearer ${T}' },
      settings({ proxy: 'https://proxy.test/?url=', variables: { V: 'openapi', T: 'tok' } }),
      fetchFn,
    );
    expect(text).toBe('spec');
    expect(fetchFn).toHaveBeenCalledWith('https://proxy.test/?url=https://api.test/openapi.json', {
      headers: { Authorization: 'Bearer tok' },
    });
  });

  it('classifies network failures as CORS problems', async () => {
    const fetchFn = async () => {
      throw new TypeError('Failed to fetch');
    };
    await expect(fetchText('https://api.test/a', {}, settings(), fetchFn)).rejects.toMatchObject({
      kind: 'cors',
      message: expect.stringMatching(/CORS/),
    });
  });

  it('classifies non-2xx responses as HTTP problems', async () => {
    const fetchFn = async () => new Response('no', { status: 404 });
    await expect(fetchText('https://api.test/a', {}, settings(), fetchFn)).rejects.toMatchObject({
      kind: 'http',
      message: expect.stringMatching(/404/),
    });
  });
});

describe('memoizeLoader', () => {
  it('caches successes and retries failures', async () => {
    let calls = 0;
    const load = memoizeLoader(async () => {
      calls += 1;
      if (calls === 1) throw new Error('flaky');
      return 'ok';
    });
    await expect(load('u', {})).rejects.toThrow('flaky');
    expect(await load('u', {})).toBe('ok');
    expect(await load('u', {})).toBe('ok');
    expect(calls).toBe(2);
  });
});

describe('resolveSources', () => {
  const files = [{ name: 'specs/a.yaml', content: 'A' }];

  it('maps file sources to workspace files and url sources to the loader, in order', async () => {
    const load = vi.fn(async () => 'REMOTE');
    const result = await resolveSources([urlSource('https://x.test/o.json'), fileSource('./specs/a.yaml')], files, load);
    expect(result).toEqual({ contents: ['REMOTE', 'A'], problems: [] });
    expect(load).toHaveBeenCalledWith('https://x.test/o.json', { Authorization: 'Bearer ${T}' });
  });

  it('reports every failing source, in source order, and returns no contents', async () => {
    const load = async () => {
      throw new Error('boom');
    };
    const result = await resolveSources([fileSource('missing.yaml', 'm'), urlSource('https://x.test', 'r')], files, load);
    expect(result.contents).toBeNull();
    expect(result.problems.map((p) => [p.source, p.kind])).toEqual([
      ['m', 'missing-file'],
      ['r', 'http'],
    ]);
    expect(result.problems[0].message).toContain("No workspace file named 'missing.yaml'");
  });
});
