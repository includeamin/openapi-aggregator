export interface WorkspaceFile {
  name: string;
  content: string;
}

export interface Settings {
  proxy: string;
  variables: Record<string, string>;
}

export interface SourceSummary {
  name: string;
  kind: 'file' | 'url';
  path: string | null;
  url: string | null;
  headers: Record<string, string>;
}

export interface ConfigSummary {
  sources: SourceSummary[];
  format: 'yaml' | 'json';
}

export interface MissingDocument {
  location: string;
  referencedFrom: string;
}

/** For each source (same order as the config), the external documents it still needs. */
export interface PendingRefs {
  sources: MissingDocument[][];
}

export interface Finding {
  severity: 'error' | 'warning';
  code: string;
  message: string;
  /** JSON pointer into the merged spec. */
  location: string;
}

export interface AggregateOutput {
  text: string;
  warnings: string[];
  findings: Finding[];
}

export type ProblemKind = 'config' | 'missing-file' | 'variable' | 'cors' | 'http' | 'merge' | 'validation' | 'blocked';

export interface Problem {
  level: 'error' | 'warning';
  kind: ProblemKind;
  message: string;
  source?: string;
  line?: number;
}
