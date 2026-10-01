import { describe, expect, it } from 'vitest';
import { fileNameFromUrl, isNameTaken, uniqueFileName, upsertFile, workspaceKey } from '../src/workspace';

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

describe('uniqueFileName', () => {
  it('keeps a free name and suffixes a taken one before the extension', () => {
    const files = [
      { name: 'specs/new.yaml', content: '' },
      { name: 'specs/new-2.yaml', content: '' },
    ];
    expect(uniqueFileName(files, 'specs/other.yaml')).toBe('specs/other.yaml');
    expect(uniqueFileName(files, './specs/new.yaml')).toBe('specs/new-3.yaml');
    expect(uniqueFileName([{ name: 'README', content: '' }], 'README')).toBe('README-2');
  });
});

describe('isNameTaken', () => {
  it('detects another file with the same key, ignoring the file being renamed', () => {
    const files = [
      { name: 'specs/a.yaml', content: '' },
      { name: 'specs/b.yaml', content: '' },
    ];
    expect(isNameTaken(files, 1, './specs/a.yaml')).toBe(true);
    expect(isNameTaken(files, 0, 'specs/a.yaml')).toBe(false);
    expect(isNameTaken(files, 1, 'specs/c.yaml')).toBe(false);
  });
});
