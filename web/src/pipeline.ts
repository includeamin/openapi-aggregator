import { errorMessage, resolveSources, type Loader } from './resolve';
import type { AggregateOutput, ConfigSummary, Problem, WorkspaceFile } from './types';

export interface PipelineResult {
  text: string | null;
  format: 'yaml' | 'json';
  problems: Problem[];
}

export interface PipelineDeps {
  parseConfig(yaml: string): ConfigSummary;
  aggregate(yaml: string, contents: string[]): AggregateOutput;
  load: Loader;
}

export function lineOf(message: string): number | undefined {
  const match = /line (\d+)/.exec(message);
  return match ? Number(match[1]) : undefined;
}

/** Config → resolve sources → merge. Never throws; every failure becomes a Problem. */
export async function runPipeline(config: string, files: WorkspaceFile[], deps: PipelineDeps): Promise<PipelineResult> {
  let summary: ConfigSummary;
  try {
    summary = deps.parseConfig(config);
  } catch (e) {
    const message = errorMessage(e);
    return { text: null, format: 'yaml', problems: [{ level: 'error', kind: 'config', message, line: lineOf(message) }] };
  }

  const { contents, problems } = await resolveSources(summary.sources, files, deps.load);
  if (!contents) return { text: null, format: summary.format, problems };

  try {
    const out = deps.aggregate(config, contents);
    const warnings: Problem[] = out.warnings.map((message) => ({ level: 'warning', kind: 'merge', message }));
    return { text: out.text, format: summary.format, problems: [...problems, ...warnings] };
  } catch (e) {
    return {
      text: null,
      format: summary.format,
      problems: [...problems, { level: 'error', kind: 'merge', message: errorMessage(e) }],
    };
  }
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
