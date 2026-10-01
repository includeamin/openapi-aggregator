import type { WorkspaceFile } from './types';

export interface SharePayload {
  v: 1;
  config: string;
  files: WorkspaceFile[];
}

export const MAX_SHARE_LENGTH = 8192;

async function pipe(
  bytes: Uint8Array<ArrayBuffer>,
  transform: CompressionStream | DecompressionStream,
): Promise<Uint8Array<ArrayBuffer>> {
  const stream = new Blob([bytes]).stream().pipeThrough(transform);
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

function toBase64Url(bytes: Uint8Array): string {
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const base64 = text.replace(/-/g, '+').replace(/_/g, '/');
  const binary = atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4));
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

function isPayload(value: unknown): value is SharePayload {
  const p = value as SharePayload;
  return (
    typeof p === 'object' &&
    p !== null &&
    p.v === 1 &&
    typeof p.config === 'string' &&
    Array.isArray(p.files) &&
    p.files.every((f) => typeof f?.name === 'string' && typeof f?.content === 'string')
  );
}

/** Variables and proxy settings are deliberately not part of the payload. */
export async function encodeShare(payload: SharePayload): Promise<string> {
  const json = new TextEncoder().encode(JSON.stringify(payload));
  return toBase64Url(await pipe(json, new CompressionStream('deflate-raw')));
}

export async function decodeShare(encoded: string): Promise<SharePayload> {
  const bytes = await pipe(fromBase64Url(encoded), new DecompressionStream('deflate-raw'));
  const value: unknown = JSON.parse(new TextDecoder().decode(bytes));
  if (!isPayload(value)) throw new Error('This share link is not a valid playground link.');
  return { v: 1, config: value.config, files: value.files.map(({ name, content }) => ({ name, content })) };
}

export function shareParam(hash: string): string | null {
  return hash.startsWith('#s=') ? hash.slice(3) : null;
}
