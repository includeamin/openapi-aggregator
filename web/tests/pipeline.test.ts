import { beforeAll, describe, expect, it } from 'vitest';
import { aggregate, parseConfig } from '../src/engine';
import { latestOnly, lineOf, runPipeline, type PipelineDeps } from '../src/pipeline';
import { loadEngineForTests } from './helpers';

beforeAll(loadEngineForTests);

const deps = (load: PipelineDeps['load'] = async () => 'unused'): PipelineDeps => ({ parseConfig, aggregate, load });
const spec = (title: string, paths = '  /a:\n    get: {summary: a}\n') =>
  `openapi: 3.0.3\ninfo: {title: ${title}, version: '1'}\npaths:\n${paths}`;

describe('runPipeline', () => {
  it('merges workspace files', async () => {
    const result = await runPipeline('sources:\n  - path: a.yaml\n', [{ name: 'a.yaml', content: spec('A') }], deps());
    expect(result.text).toContain('title: A');
    expect(result.problems).toEqual([]);
    expect(result.format).toBe('yaml');
  });

  it('reports config errors with a line number and no output', async () => {
    const result = await runPipeline('sources:\n  - path: a.yaml\n    bogus: 1\n', [], deps());
    expect(result.text).toBeNull();
    expect(result.problems).toHaveLength(1);
    expect(result.problems[0]).toMatchObject({ level: 'error', kind: 'config' });
    expect(result.problems[0].message).toContain('bogus');
    expect(result.problems[0].line).toBeGreaterThan(0);
  });

  it('names a Swagger 2.0 source and says only 3.x is supported', async () => {
    const result = await runPipeline(
      'sources:\n  - name: legacy\n    path: old.yaml\n',
      [{ name: 'old.yaml', content: "swagger: '2.0'\ninfo: {title: Old, version: '1'}\npaths: {}\n" }],
      deps(),
    );
    expect(result.text).toBeNull();
    expect(result.problems[0]).toMatchObject({ kind: 'merge' });
    expect(result.problems[0].message).toMatch(/legacy/);
    expect(result.problems[0].message).toMatch(/only OpenAPI 3\.x/);
  });

  it('turns merge warnings into warning problems while keeping output', async () => {
    const v31 = spec('B', '  /b:\n    get: {summary: b}\n').replace('3.0.3', '3.1.0');
    const result = await runPipeline(
      'sources:\n  - path: a.yaml\n  - path: b.yaml\n',
      [
        { name: 'a.yaml', content: spec('A') },
        { name: 'b.yaml', content: v31 },
      ],
      deps(),
    );
    expect(result.text).not.toBeNull();
    expect(result.problems).toEqual([expect.objectContaining({ level: 'warning', kind: 'merge' })]);
  });

  it('stays fast for large specs (3,000 paths)', async () => {
    const paths = Array.from({ length: 3000 }, (_, i) => `  /r${i}:\n    get: {summary: r${i}}\n`).join('');
    const started = performance.now();
    const result = await runPipeline('sources:\n  - path: big.yaml\n', [{ name: 'big.yaml', content: spec('Big', paths) }], deps());
    expect(result.problems).toEqual([]);
    expect(performance.now() - started).toBeLessThan(2000);
  });
});

describe('lineOf', () => {
  it('extracts the line from serde_yaml messages', () => {
    expect(lineOf('unknown field `bogus` at line 3 column 5')).toBe(3);
    expect(lineOf('no location')).toBeUndefined();
  });
});

describe('latestOnly', () => {
  it('drops results of superseded calls', async () => {
    const resolvers: Array<(v: string) => void> = [];
    const run = latestOnly((_: number) => new Promise<string>((resolve) => resolvers.push(resolve)));
    const first = run(1);
    const second = run(2);
    resolvers[1]('second');
    resolvers[0]('first');
    expect(await second).toBe('second');
    expect(await first).toBeUndefined();
  });
});
