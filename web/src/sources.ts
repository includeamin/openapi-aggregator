import type { PipelineResult } from './pipeline';
import type { SourceSummary, WorkspaceFile } from './types';
import { workspaceKey } from './workspace';

/** Number of distinct source colours (`--src-0` … `--src-5` in style.css). */
export const SOURCE_HUES = 6;

export interface SourceChip {
  name: string;
  kind: 'file' | 'url';
  hue: number;
}

export function sourceChips(sources: SourceSummary[]): SourceChip[] {
  return sources.map((s, i) => ({ name: s.name, kind: s.kind, hue: i % SOURCE_HUES }));
}

/** The hue of the source that reads each workspace file, or null if the config doesn't use it. */
export function fileHues(sources: SourceSummary[], files: WorkspaceFile[]): (number | null)[] {
  return files.map((f) => {
    const index = sources.findIndex((s) => s.kind === 'file' && workspaceKey(s.path ?? '') === workspaceKey(f.name));
    return index < 0 ? null : index % SOURCE_HUES;
  });
}

export type RunState = 'ok' | 'warning' | 'error' | 'blocked';

export function statusOf(result: PipelineResult): { state: RunState; label: string } {
  const kinds = new Set(result.problems.map((p) => p.kind));
  if (kinds.has('blocked')) return { state: 'blocked', label: 'Waiting for permission to fetch' };
  if (result.text === null) {
    return { state: 'error', label: kinds.has('config') ? 'Config has errors' : 'Merge failed' };
  }
  if (result.problems.length) return { state: 'warning', label: 'Merged with warnings' };
  return { state: 'ok', label: 'Merged' };
}

export function problemCountLabel(count: number): string {
  if (count === 0) return 'No problems';
  return count === 1 ? '1 problem' : `${count} problems`;
}
