import { errorMessage, resolveDocuments, resolveSources, type Loader, type WantedDocument } from './resolve';
import type { AggregateOutput, ConfigSummary, PendingRefs, Problem, WorkspaceFile } from './types';

/** Mirrors the engine's limit on external documents per config. */
const MAX_DOCUMENTS = 100;

export interface PipelineResult {
  text: string | null;
  format: 'yaml' | 'json';
  problems: Problem[];
}

export interface PipelineDeps {
  parseConfig(yaml: string, files: string[]): ConfigSummary;
  pendingRefs(yaml: string, contents: string[], docs: Record<string, string>, files: string[]): PendingRefs;
  aggregate(yaml: string, contents: string[], docs: Record<string, string>, files: string[]): AggregateOutput;
  load: Loader;
}

export function lineOf(message: string): number | undefined {
  const match = /line (\d+)/.exec(message);
  return match ? Number(match[1]) : undefined;
}

/** Config → resolve sources → merge. Never throws; every failure becomes a Problem. */
export async function runPipeline(config: string, files: WorkspaceFile[], deps: PipelineDeps): Promise<PipelineResult> {
  // Glob sources are matched against the workspace file names.
  const fileNames = files.map((f) => f.name);
  let summary: ConfigSummary;
  try {
    summary = deps.parseConfig(config, fileNames);
  } catch (e) {
    const message = errorMessage(e);
    return { text: null, format: 'yaml', problems: [{ level: 'error', kind: 'config', message, line: lineOf(message) }] };
  }

  const { contents, problems } = await resolveSources(summary.sources, files, deps.load);
  if (!contents) return { text: null, format: summary.format, problems };

  const docs: Record<string, string> = {};
  try {
    for (;;) {
      const wanted = collectWanted(deps.pendingRefs(config, contents, docs, fileNames), summary);
      if (!wanted.length) break;
      if (Object.keys(docs).length + wanted.length > MAX_DOCUMENTS) {
        throw new Error(`too many external documents (limit ${MAX_DOCUMENTS})`);
      }
      const loaded = await resolveDocuments(wanted, files, deps.load);
      if (loaded.problems.length) return { text: null, format: summary.format, problems: loaded.problems };
      Object.assign(docs, loaded.docs);
    }
  } catch (e) {
    return { text: null, format: summary.format, problems: [{ level: 'error', kind: 'merge', message: errorMessage(e) }] };
  }

  try {
    const out = deps.aggregate(config, contents, docs, fileNames);
    const warnings: Problem[] = out.warnings.map((message) => ({ level: 'warning', kind: 'merge', message }));
    const findings: Problem[] = out.findings.map((f) => ({
      level: f.severity,
      kind: 'validation',
      message: `${f.message} (${f.location || '/'})`,
    }));
    return { text: out.text, format: summary.format, problems: [...problems, ...warnings, ...findings] };
  } catch (e) {
    return {
      text: null,
      format: summary.format,
      problems: [...problems, { level: 'error', kind: 'merge', message: errorMessage(e) }],
    };
  }
}

/** Flatten per-source pending documents, keeping the first source that needs each location. */
function collectWanted(pending: PendingRefs, summary: ConfigSummary): WantedDocument[] {
  const seen = new Set<string>();
  const wanted: WantedDocument[] = [];
  pending.sources.forEach((missing, index) => {
    for (const doc of missing) {
      if (seen.has(doc.location)) continue;
      seen.add(doc.location);
      wanted.push({ ...doc, source: summary.sources[index] });
    }
  });
  return wanted;
}

/** Wrap an async function so only the most recent call's result is delivered. */
export function latestOnly<A extends unknown[], R>(fn: (...args: A) => Promise<R>): (...args: A) => Promise<R | undefined> {
  let latest = 0;
  return async (...args: A) => {
    const id = ++latest;
    const result = await fn(...args);
    return id === latest ? result : undefined;
  };
}
