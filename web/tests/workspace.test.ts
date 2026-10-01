import { describe, expect, it } from 'vitest';
import { fileNameFromUrl, upsertFile, workspaceKey } from '../src/workspace';

describe('workspaceKey', () => {
  it('strips leading ./ segments', () => {
    expect(workspaceKey('./specs/a.yaml')).toBe('specs/a.yaml');
    expect(workspaceKey('././a.yaml')).toBe('a.yaml');
    expect(workspaceKey('specs/a.yaml')).toBe('specs/a.yaml');
  });
});

describe('upsertFile', () => {
  it('replaces the content of a file with the same key instead of duplicating it', () => {
    const files = [{ name: 'specs/a.yaml', content: 'old' }];
    const index = upsertFile(files, { name: './specs/a.yaml', content: 'new' });
    expect(index).toBe(0);
    expect(files).toEqual([{ name: 'specs/a.yaml', content: 'new' }]);
  });

  it('appends new files', () => {
    const files = [{ name: 'a.yaml', content: '' }];
    expect(upsertFile(files, { name: 'b.yaml', content: '' })).toBe(1);
    expect(files).toHaveLength(2);
  });
});

describe('fileNameFromUrl', () => {
  it('uses the last path segment', () => {
    expect(fileNameFromUrl('https://x.test/v1/openapi.json?x=1')).toBe('openapi.json');
  });
  it('falls back for bare hosts and invalid urls', () => {
    expect(fileNameFromUrl('https://x.test/')).toBe('imported.yaml');
    expect(fileNameFromUrl('not a url')).toBe('imported.yaml');
  });
});
