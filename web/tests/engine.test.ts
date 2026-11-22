import { beforeAll, describe, expect, it } from 'vitest';
import { aggregate, parseConfig, pendingRefs } from '../src/engine';
import { loadEngineForTests } from './helpers';

const PETS = "openapi: 3.0.3\ninfo: {title: Pets, version: '1'}\npaths:\n  /pets:\n    get: {summary: list}\n";

beforeAll(loadEngineForTests);

describe('engine', () => {
  it('summarizes the config as plain objects', () => {
    const summary = parseConfig('sources:\n  - path: ./pets.yaml\n');
    expect(summary).toEqual({
      sources: [{ name: 'pets', kind: 'file', path: './pets.yaml', url: null, headers: {} }],
      format: 'yaml',
    });
  });

  it('throws an Error for an invalid config', () => {
    expect(() => parseConfig('sources: [{name: x}]')).toThrowError(/either 'path' or 'url'/);
  });

  it('aggregates loaded contents', () => {
    const out = aggregate('sources:\n  - path: pets.yaml\n', [PETS]);
    expect(out.text).toContain('/pets:');
    expect(out.warnings).toEqual([]);
  });

  it('lists the external documents a source still needs, then bundles them', () => {
    const root = "openapi: 3.0.3\ninfo: {title: R, version: '1'}\npaths: {}\ncomponents:\n  schemas:\n    A: {$ref: './a.yaml#/A'}\n";
    const config = 'sources:\n  - path: ./root.yaml\n';
    expect(pendingRefs(config, [root], {})).toEqual({ sources: [[{ location: 'a.yaml', referencedFrom: 'root.yaml' }]] });
    expect(pendingRefs(config, [root], { 'a.yaml': 'A:\n  type: string\n' })).toEqual({ sources: [[]] });
    expect(aggregate(config, [root], { 'a.yaml': 'A:\n  type: string\n' }).text).toContain('type: string');
  });

  it('expands glob sources against the workspace file names', () => {
    const config = 'sources:\n  - path: ./specs/*.yaml\n';
    const summary = parseConfig(config, ['specs/b.yaml', 'specs/a.yaml', 'other.yaml']);
    expect(summary.sources.map((s) => [s.name, s.path])).toEqual([
      ['a', 'specs/a.yaml'],
      ['b', 'specs/b.yaml'],
    ]);
    expect(() => parseConfig(config, ['other.yaml'])).toThrow(/matched no files/);
    // configs without globs do not need a file list
    expect(parseConfig('sources:\n  - path: ./pets.yaml\n').sources).toHaveLength(1);
  });
});
