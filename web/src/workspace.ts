import type { WorkspaceFile } from './types';

/** Normalise a config `path:` or file name so `./specs/a.yaml` matches `specs/a.yaml`. */
export function workspaceKey(path: string): string {
  return path.replace(/^(\.\/)+/, '');
}

/** Insert `file`, or replace the content of the file with the same key. Returns its index. */
export function upsertFile(files: WorkspaceFile[], file: WorkspaceFile): number {
  const key = workspaceKey(file.name);
  const index = files.findIndex((f) => workspaceKey(f.name) === key);
  if (index >= 0) {
    files[index].content = file.content;
    return index;
  }
  files.push({ name: key, content: file.content });
  return files.length - 1;
}

export function fileNameFromUrl(url: string): string {
  try {
    const last = new URL(url).pathname.split('/').filter(Boolean).pop();
    return last || 'imported.yaml';
  } catch {
    return 'imported.yaml';
  }
}
