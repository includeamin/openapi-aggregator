import { describe, expect, it } from 'vitest';
import { decodeShare, encodeShare, shareParam, type SharePayload } from '../src/share';

const payload: SharePayload = {
  v: 1,
  config: 'sources:\n  - path: specs/a.yaml\n',
  files: [{ name: 'specs/a.yaml', content: 'openapi: 3.0.3\ninfo: {title: 日本語 API 🚀, version: "1"}\npaths: {}\n' }],
};

describe('share links', () => {
  it('round-trips, including non-ASCII text', async () => {
    const encoded = await encodeShare(payload);
    expect(encoded).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(await decodeShare(encoded)).toEqual(payload);
  });

  it('compresses repetitive specs', async () => {
    const big = { ...payload, files: [{ name: 'a.yaml', content: 'paths:\n' + '  /x: {}\n'.repeat(2000) }] };
    expect((await encodeShare(big)).length).toBeLessThan(2000);
  });

  it('rejects garbage and wrong shapes', async () => {
    await expect(decodeShare('!!!')).rejects.toThrow();
    const wrong = await encodeShare({ v: 2, config: 1 } as unknown as SharePayload);
    await expect(decodeShare(wrong)).rejects.toThrow(/share link/);
  });

  it('extracts the s= hash parameter', () => {
    expect(shareParam('#s=abc')).toBe('abc');
    expect(shareParam('#other')).toBeNull();
    expect(shareParam('')).toBeNull();
  });
});
