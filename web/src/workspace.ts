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

/** `name` if no file uses that key yet, otherwise `name-2`, `name-3`, … (before the extension). */
export function uniqueFileName(files: WorkspaceFile[], name: string): string {
  const key = workspaceKey(name);
  const taken = new Set(files.map((f) => workspaceKey(f.name)));
  if (!taken.has(key)) return key;
  const dot = key.lastIndexOf('.');
  const slash = key.lastIndexOf('/');
  const [stem, ext] = dot > slash ? [key.slice(0, dot), key.slice(dot)] : [key, ''];
  let n = 2;
  while (taken.has(`${stem}-${n}${ext}`)) n += 1;
  return `${stem}-${n}${ext}`;
}

/** Whether a file other than `files[index]` already uses `name`'s key. */
export function isNameTaken(files: WorkspaceFile[], index: number, name: string): boolean {
  const key = workspaceKey(name);
  return files.some((f, i) => i !== index && workspaceKey(f.name) === key);
}

export function fileNameFromUrl(url: string): string {
  try {
    const last = new URL(url).pathname.split('/').filter(Boolean).pop();
    return last || 'imported.yaml';
  } catch {
    return 'imported.yaml';
  }
}
