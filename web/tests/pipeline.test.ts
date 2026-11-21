import { beforeAll, describe, expect, it } from 'vitest';
import { aggregate, parseConfig, pendingRefs } from '../src/engine';
import { SourceError } from '../src/resolve';
import { latestOnly, lineOf, runPipeline, type PipelineDeps } from '../src/pipeline';
import { loadEngineForTests } from './helpers';

beforeAll(loadEngineForTests);

const deps = (load: PipelineDeps['load'] = async () => 'unused'): PipelineDeps => ({ parseConfig, aggregate, pendingRefs, load });
const spec = (title: string, paths = '  /a:\n    get: {summary: a, responses: {"200": {description: ok}}}\n') =>
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

  it('converts a Swagger 2.0 source and reports a warning', async () => {
    const result = await runPipeline(
      'sources:\n  - name: legacy\n    path: old.yaml\n',
      [{ name: 'old.yaml', content: "swagger: '2.0'\ninfo: {title: Old, version: '1'}\npaths: {}\n" }],
      deps(),
    );
    expect(result.text).toContain('openapi: 3.0.3');
    expect(result.problems).toEqual([
      expect.objectContaining({ level: 'warning', kind: 'merge', message: expect.stringContaining("source 'legacy': converted from Swagger 2.0") }),
    ]);
  });

  it('rejects Swagger 1.x sources by name', async () => {
    const result = await runPipeline(
      'sources:\n  - name: ancient\n    path: old.yaml\n',
      [{ name: 'old.yaml', content: "swagger: '1.2'\ninfo: {title: Old, version: '1'}\n" }],
      deps(),
    );
    expect(result.text).toBeNull();
    expect(result.problems[0].message).toMatch(/ancient/);
    expect(result.problems[0].message).toMatch(/Swagger 1\.x/);
  });

  it('turns merge warnings into warning problems while keeping output', async () => {
    const v31 = spec('B', '  /b:\n    get: {summary: b, responses: {"200": {description: ok}}}\n').replace('3.0.3', '3.1.0');
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
    const paths = Array.from({ length: 3000 }, (_, i) => `  /r${i}:\n    get: {summary: r${i}, responses: {"200": {description: ok}}}\n`).join('');
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

describe('external refs', () => {
  const root = (ref: string) =>
    `openapi: 3.0.3\ninfo: {title: Root, version: '1'}\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: '${ref}'\n`;
  const config = 'sources:\n  - name: root\n    path: specs/root.yaml\n';

  it('bundles refs resolved from workspace files, over several rounds', async () => {
    const result = await runPipeline(
      config,
      [
        { name: 'specs/root.yaml', content: root('./schemas/pet.yaml#/Pet') },
        { name: 'specs/schemas/pet.yaml', content: "Pet:\n  type: object\n  properties:\n    owner:\n      $ref: './owner.yaml#/Owner'\n" },
        { name: 'specs/schemas/owner.yaml', content: 'Owner:\n  type: string\n' },
      ],
      deps(),
    );
    expect(result.problems).toEqual([]);
    expect(result.text).toContain('Owner:');
    expect(result.text).toContain("'#/components/schemas/Owner'");
  });

  it('names the missing workspace file and who referenced it', async () => {
    const result = await runPipeline(config, [{ name: 'specs/root.yaml', content: root('./schemas/pet.yaml#/Pet') }], deps());
    expect(result.text).toBeNull();
    expect(result.problems).toEqual([
      expect.objectContaining({
        level: 'error',
        kind: 'missing-file',
        source: 'root',
        message:
          "No workspace file named 'specs/schemas/pet.yaml' (referenced from 'specs/root.yaml'). Add it to the workspace with exactly that name.",
      }),
    ]);
  });

  it('reports a blocked remote ref instead of fetching', async () => {
    const result = await runPipeline(
      config,
      [{ name: 'specs/root.yaml', content: root('https://schemas.test/pet.yaml#/Pet') }],
      deps(async () => {
        throw new SourceError('blocked', 'Not fetched: https://schemas.test/pet.yaml.');
      }),
    );
    expect(result.text).toBeNull();
    expect(result.problems[0]).toMatchObject({ level: 'error', kind: 'blocked' });
  });

  it('sends source headers only to the same origin', async () => {
    const urlRoot =
      "openapi: 3.0.3\ninfo: {title: R, version: '1'}\npaths: {}\ncomponents:\n  schemas:\n    A: {$ref: './local.yaml#/Local'}\n    B: {$ref: 'https://b.test/x.yaml#/Foreign'}\n";
    const calls: Array<[string, Record<string, string>]> = [];
    const load = async (url: string, headers: Record<string, string>) => {
      calls.push([url, headers]);
      if (url.endsWith('/spec.yaml')) return urlRoot;
      return url.includes('local') ? 'Local:\n  type: string\n' : 'Foreign:\n  type: integer\n';
    };
    const result = await runPipeline(
      'sources:\n  - name: r\n    url: https://a.test/api/spec.yaml\n    headers: {Authorization: Bearer t}\n',
      [],
      deps(load),
    );
    expect(result.problems).toEqual([]);
    expect(calls.find(([u]) => u === 'https://a.test/api/local.yaml')?.[1]).toEqual({ Authorization: 'Bearer t' });
    expect(calls.find(([u]) => u === 'https://b.test/x.yaml')?.[1]).toEqual({});
  });
});

describe('validation findings', () => {
  const dangling = spec('V', '  /v:\n    get:\n      responses:\n        "200":\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: "#/components/schemas/Missing"\n');

  it('shows output together with validation problems', async () => {
    const result = await runPipeline('sources:\n  - path: v.yaml\n', [{ name: 'v.yaml', content: dangling }], deps());
    expect(result.text).not.toBeNull();
    expect(result.problems).toEqual([
      expect.objectContaining({ level: 'error', kind: 'validation', message: expect.stringContaining('Missing') }),
    ]);
    expect(result.problems[0].message).toContain('/paths/~1v/get/responses/200/content/application~1json/schema');
  });

  it('reports no validation problems for a clean spec', async () => {
    const result = await runPipeline('sources:\n  - path: a.yaml\n', [{ name: 'a.yaml', content: spec('A') }], deps());
    expect(result.problems.filter((p) => p.kind === 'validation')).toEqual([]);
  });
});
