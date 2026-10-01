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

export interface AggregateOutput {
  text: string;
  warnings: string[];
}

export type ProblemKind = 'config' | 'missing-file' | 'variable' | 'cors' | 'http' | 'merge' | 'blocked';

export interface Problem {
  level: 'error' | 'warning';
  kind: ProblemKind;
  message: string;
  source?: string;
  line?: number;
}
