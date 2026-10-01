import { describe, expect, it } from 'vitest';
import { fileHues, problemCountLabel, sourceChips, statusOf } from '../src/sources';
import type { PipelineResult } from '../src/pipeline';
import type { SourceSummary } from '../src/types';

const file = (name: string, path: string): SourceSummary => ({ name, kind: 'file', path, url: null, headers: {} });
const url = (name: string): SourceSummary => ({ name, kind: 'url', path: null, url: 'https://x.test/a', headers: {} });

describe('source colours', () => {
  const sources = [file('inventory', './specs/inventory.yaml'), url('petstore'), file('catalog', 'specs/catalog.yaml')];

  it('gives each source a hue by position, cycling after six', () => {
    expect(sourceChips(sources)).toEqual([
      { name: 'inventory', kind: 'file', hue: 0 },
      { name: 'petstore', kind: 'url', hue: 1 },
      { name: 'catalog', kind: 'file', hue: 2 },
    ]);
    const many = Array.from({ length: 7 }, (_, i) => file(`s${i}`, `s${i}.yaml`));
    expect(sourceChips(many)[6].hue).toBe(0);
  });

  it('colours workspace files by the source that uses them, null when unused', () => {
    const files = [
      { name: 'specs/catalog.yaml', content: '' },
      { name: 'notes.yaml', content: '' },
      { name: 'specs/inventory.yaml', content: '' },
    ];
    expect(fileHues(sources, files)).toEqual([2, null, 0]);
  });
});

describe('status', () => {
  const result = (over: Partial<PipelineResult>): PipelineResult => ({ text: 'x', format: 'yaml', problems: [], ...over });

  it('describes the outcome of the last run', () => {
    expect(statusOf(result({}))).toEqual({ state: 'ok', label: 'Merged' });
    expect(statusOf(result({ problems: [{ level: 'warning', kind: 'merge', message: 'w' }] }))).toEqual({
      state: 'warning',
      label: 'Merged with warnings',
    });
    expect(statusOf(result({ text: null, problems: [{ level: 'error', kind: 'config', message: 'c' }] }))).toEqual({
      state: 'error',
      label: 'Config has errors',
    });
    expect(statusOf(result({ text: null, problems: [{ level: 'error', kind: 'merge', message: 'm' }] }))).toEqual({
      state: 'error',
      label: 'Merge failed',
    });
    expect(statusOf(result({ text: null, problems: [{ level: 'error', kind: 'blocked', message: 'b' }] }))).toEqual({
      state: 'blocked',
      label: 'Waiting for permission to fetch',
    });
  });

  it('counts problems in words', () => {
    expect(problemCountLabel(0)).toBe('No problems');
    expect(problemCountLabel(1)).toBe('1 problem');
    expect(problemCountLabel(3)).toBe('3 problems');
  });
});
