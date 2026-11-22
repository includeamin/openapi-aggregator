import { beforeAll, describe, expect, it } from 'vitest';
import { aggregate, parseConfig, pendingRefs } from '../src/engine';
import { runPipeline } from '../src/pipeline';
import { DEFAULT_EXAMPLE_ID, EXAMPLES, findExample } from '../src/examples';
import { resolveSources } from '../src/resolve';
import { loadEngineForTests } from './helpers';

// Loaded at collection time too: parseConfigSafe() below runs while tests are being defined.
loadEngineForTests();
beforeAll(loadEngineForTests);

const noNetwork = async () => {
  throw new Error('network not allowed in tests');
};

describe('examples', () => {
  it('has unique ids and the default exists', () => {
    expect(new Set(EXAMPLES.map((e) => e.id)).size).toBe(EXAMPLES.length);
    expect(findExample(DEFAULT_EXAMPLE_ID).id).toBe('rename');
    expect(findExample('nope').id).toBe('rename');
  });

  for (const example of EXAMPLES) {
    const noValidationErrors = async () => {
      const result = await runPipeline(example.config, example.files, { parseConfig, aggregate, pendingRefs, load: noNetwork });
      return result.problems.filter((p) => p.kind === 'validation' && p.level === 'error');
    };
    const remote = parseConfigSafe(example.config, example.files.map((f) => f.name)).some((s) => s.kind === 'url');
    it.skipIf(remote)(`${example.id}: merged output has no validation errors`, async () => {
      expect(await noValidationErrors()).toEqual([]);
    });

    it(`${example.id}: config parses`, () => {
      expect(() => parseConfig(example.config, example.files.map((f) => f.name))).not.toThrow();
    });

    const usesNetwork = parseConfigSafe(example.config, example.files.map((f) => f.name)).some((s) => s.kind === 'url');
    const expectsWarnings = example.id === 'swagger2';
    it.skipIf(usesNetwork || expectsWarnings)(`${example.id}: merges without errors or warnings`, async () => {
      const result = await runPipeline(example.config, example.files, { parseConfig, aggregate, pendingRefs, load: noNetwork });
      expect(result.problems).toEqual([]);
      expect(result.text!.length).toBeGreaterThan(0);
    });
  }

  it('rename example shows renamed paths, components and refs', async () => {
    const ex = findExample('rename');
    const { contents } = await resolveSources(parseConfig(ex.config).sources, ex.files, noNetwork);
    const text = aggregate(ex.config, contents!).text;
    expect(text).toContain('/catalog/items:');
    expect(text).toContain('catalog_Item:');
    expect(text).toContain("'#/components/schemas/catalog_Item'");
  });

  it('swagger2 example converts the legacy spec and says so', async () => {
    const ex = findExample('swagger2');
    expect(ex.id).toBe('swagger2');
    const { contents, problems } = await resolveSources(parseConfig(ex.config).sources, ex.files, noNetwork);
    expect(problems).toEqual([]);
    const out = aggregate(ex.config, contents!);
    expect(out.text).toContain('openapi: 3.0.3');
    expect(out.text).toContain('requestBody:');
    expect(out.warnings.some((w) => w.includes('converted from Swagger 2.0'))).toBe(true);
  });

  it('multi-file example bundles refs from workspace files', async () => {
    const ex = findExample('multi-file');
    expect(ex.id).toBe('multi-file');
    const result = await runPipeline(ex.config, ex.files, { parseConfig, aggregate, pendingRefs, load: noNetwork });
    expect(result.problems).toEqual([]);
    expect(result.text).toContain('components:');
    expect(result.text).toMatch(/\$ref: '#\/components\/schemas\/\w+'/);
    expect(result.text).not.toMatch(/\.yaml#/);
  });

  it('hide-internal example removes internal endpoints and everything only they used', async () => {
    const ex = findExample('hide-internal');
    expect(ex.id).toBe('hide-internal');
    const result = await runPipeline(ex.config, ex.files, { parseConfig, aggregate, pendingRefs, load: noNetwork });
    expect(result.problems).toEqual([]);
    const text = result.text!;
    expect(text).toContain('/invoices:');
    expect(text).toContain('/users:');
    expect(text).not.toContain('/internal');
    expect(text).not.toContain('/debug');
    expect(text).not.toContain('/audit');
    expect(text).not.toMatch(/internal/i);
    expect(text).not.toContain('AuditEntry');
    expect(text).not.toContain('Metrics');
  });

  it('glob example expands the pattern from the workspace files', async () => {
    const ex = findExample('glob');
    expect(ex.id).toBe('glob');
    const result = await runPipeline(ex.config, ex.files, { parseConfig, aggregate, pendingRefs, load: noNetwork });
    expect(result.problems).toEqual([]);
    for (const path of ['/users:', '/orders:', '/billing:']) expect(result.text).toContain(path);
  });

  it('per-operation example combines GET and POST on one path', async () => {
    const ex = findExample('per-operation');
    const { contents } = await resolveSources(parseConfig(ex.config).sources, ex.files, noNetwork);
    const text = aggregate(ex.config, contents!).text;
    expect(text).toMatch(/\/users:\n\s+get:[\s\S]*\n\s+post:/);
  });
});

function parseConfigSafe(config: string, files: string[]) {
  try {
    return parseConfig(config, files).sources;
  } catch {
    return [];
  }
}
