import { workspaceKey } from './workspace';
import type { Problem, ProblemKind, Settings, SourceSummary, WorkspaceFile } from './types';

export type Loader = (url: string, headers: Record<string, string>) => Promise<string>;
export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>;

export class SourceError extends Error {
  constructor(
    readonly kind: ProblemKind,
    message: string,
  ) {
    super(message);
  }
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function substituteVars(text: string, vars: Record<string, string>): string {
  return text.replace(/\$\{([^}]*)\}/g, (_, name: string) => {
    if (!Object.hasOwn(vars, name)) {
      throw new SourceError('variable', `Variable '${name}' is not set. Add it under Settings → Variables.`);
    }
    return vars[name];
  });
}

export async function fetchText(
  url: string,
  headers: Record<string, string>,
  settings: Settings,
  fetchFn: FetchFn = (input, init) => fetch(input, init),
): Promise<string> {
  const target = substituteVars(url, settings.variables);
  const finalHeaders = Object.fromEntries(
    Object.entries(headers).map(([key, value]) => [key, substituteVars(value, settings.variables)]),
  );
  let response: Response;
  try {
    response = await fetchFn(settings.proxy + target, { headers: finalHeaders });
  } catch {
    throw new SourceError(
      'cors',
      `Could not fetch ${target}. The server is unreachable or does not allow cross-origin requests (CORS). ` +
        'Paste or upload the spec instead, or set a CORS proxy under Settings.',
    );
  }
  if (!response.ok) {
    throw new SourceError('http', `${target} returned HTTP ${response.status}.`);
  }
  return response.text();
}

/** Cache successful loads per (url, headers); failed loads are retried next time. */
export function memoizeLoader(load: Loader): Loader {
  const cache = new Map<string, Promise<string>>();
  return (url, headers) => {
    const key = JSON.stringify([url, headers]);
    let hit = cache.get(key);
    if (!hit) {
      hit = load(url, headers);
      cache.set(key, hit);
      hit.catch(() => cache.delete(key));
    }
    return hit;
  };
}

export async function resolveSources(
  sources: SourceSummary[],
  files: WorkspaceFile[],
  load: Loader,
): Promise<{ contents: string[] | null; problems: Problem[] }> {
  const results = await Promise.all(
    sources.map(async (source): Promise<{ content: string } | { problem: Problem }> => {
      try {
        if (source.kind === 'file') {
          const key = workspaceKey(source.path ?? '');
          const file = files.find((f) => workspaceKey(f.name) === key);
          if (!file) {
            throw new SourceError('missing-file', `No workspace file named '${key}'. Add it to the workspace or fix the path.`);
          }
          return { content: file.content };
        }
        return { content: await load(source.url ?? '', source.headers) };
      } catch (e) {
        const kind = e instanceof SourceError ? e.kind : 'http';
        return { problem: { level: 'error', kind, source: source.name, message: errorMessage(e) } };
      }
    }),
  );

  const problems = results.flatMap((r) => ('problem' in r ? [r.problem] : []));
  if (problems.length) return { contents: null, problems };
  return { contents: results.map((r) => (r as { content: string }).content), problems };
}
