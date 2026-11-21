import { describe, expect, it, vi } from 'vitest';
import { fetchText, guardLoader, memoizeLoader, resolveDocuments, resolveSources, substituteVars } from '../src/resolve';
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

describe('fetchText error messages', () => {
  it('show the URL template, never substituted variable values', async () => {
    const vars = settings({ variables: { T: 'top-secret' } });
    const network = async () => {
      throw new TypeError('Failed to fetch');
    };
    const notFound = async () => new Response('', { status: 404 });
    for (const fetchFn of [network, notFound]) {
      const error = (await fetchText('https://api.test/spec?t=${T}', {}, vars, fetchFn).catch((e: unknown) => e)) as Error;
      expect(error.message).not.toContain('top-secret');
      expect(error.message).toContain('${T}');
    }
  });
});

describe('guardLoader', () => {
  it('blocks remote loads until allowed, without calling the inner loader', async () => {
    let allowed = false;
    const inner = vi.fn(async () => 'spec');
    const load = guardLoader(inner, () => allowed);

    await expect(load('https://x.test/a', {})).rejects.toMatchObject({ kind: 'blocked' });
    expect(inner).not.toHaveBeenCalled();

    allowed = true;
    expect(await load('https://x.test/a', {})).toBe('spec');
  });
});

describe('resolveDocuments', () => {
  const urlRoot: SourceSummary = { name: 'r', kind: 'url', path: null, url: 'https://a.test/spec.yaml', headers: { Authorization: 'Bearer t' } };
  const file = (name: string, content = 'X: {}') => ({ name, content });

  it('reads workspace files by normalised key', async () => {
    const { docs, problems } = await resolveDocuments(
      [{ location: 'specs/a.yaml', referencedFrom: 'root.yaml', source: fileSource('root.yaml') }],
      [file('./specs/a.yaml', 'A: {}')],
      async () => 'unused',
    );
    expect(problems).toEqual([]);
    expect(docs).toEqual({ 'specs/a.yaml': 'A: {}' });
  });

  it('reports every missing file, not just the first', async () => {
    const { problems } = await resolveDocuments(
      [
        { location: 'a.yaml', referencedFrom: 'root.yaml', source: fileSource('root.yaml') },
        { location: 'b.yaml', referencedFrom: 'a.yaml', source: fileSource('root.yaml') },
      ],
      [],
      async () => 'unused',
    );
    expect(problems.map((p) => p.kind)).toEqual(['missing-file', 'missing-file']);
    expect(problems[1].message).toContain("'b.yaml' (referenced from 'a.yaml')");
  });

  it('sends headers only to the source origin and rejects file: refs', async () => {
    const seen: Array<[string, Record<string, string>]> = [];
    const load = async (url: string, headers: Record<string, string>) => {
      seen.push([url, headers]);
      return 'X: {}';
    };
    const { problems } = await resolveDocuments(
      [
        { location: 'https://a.test/d.yaml', referencedFrom: 'https://a.test/spec.yaml', source: urlRoot },
        { location: 'https://b.test/d.yaml', referencedFrom: 'https://a.test/spec.yaml', source: urlRoot },
        { location: 'file:///etc/passwd', referencedFrom: 'https://a.test/spec.yaml', source: urlRoot },
      ],
      [],
      load,
    );
    expect(seen).toEqual([
      ['https://a.test/d.yaml', { Authorization: 'Bearer t' }],
      ['https://b.test/d.yaml', {}],
    ]);
    expect(problems).toHaveLength(1);
    expect(problems[0].message).toContain('file:');
  });
});
