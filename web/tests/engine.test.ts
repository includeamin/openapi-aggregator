import { beforeAll, describe, expect, it } from 'vitest';
import { aggregate, parseConfig } from '../src/engine';
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
});
