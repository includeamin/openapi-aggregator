import { beforeAll, describe, expect, it } from 'vitest';
import { aggregate, parseConfig } from '../src/engine';
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
    it(`${example.id}: config parses`, () => {
      expect(() => parseConfig(example.config)).not.toThrow();
    });

    const usesNetwork = parseConfigSafe(example.config).some((s) => s.kind === 'url');
    const expectsWarnings = example.id === 'swagger2';
    it.skipIf(usesNetwork || expectsWarnings)(`${example.id}: merges without errors or warnings`, async () => {
      const summary = parseConfig(example.config);
      const { contents, problems } = await resolveSources(summary.sources, example.files, noNetwork);
      expect(problems).toEqual([]);
      const out = aggregate(example.config, contents!);
      expect(out.warnings).toEqual([]);
      expect(out.text.length).toBeGreaterThan(0);
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

  it('per-operation example combines GET and POST on one path', async () => {
    const ex = findExample('per-operation');
    const { contents } = await resolveSources(parseConfig(ex.config).sources, ex.files, noNetwork);
    const text = aggregate(ex.config, contents!).text;
    expect(text).toMatch(/\/users:\n\s+get:[\s\S]*\n\s+post:/);
  });
});

function parseConfigSafe(config: string) {
  try {
    return parseConfig(config).sources;
  } catch {
    return [];
  }
}
