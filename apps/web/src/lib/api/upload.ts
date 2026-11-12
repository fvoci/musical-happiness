/**
 * Uploading to the server in parts (`docs/contract-pending.md`, 4): an
 * upload is opened with the file's name and size (`createUpload`, or
 * `createImportUpload` for an archive), each part is sent with the SHA-256
 * of its bytes (`putUploadPart`), and the upload is ended (`completeUpload`
 * into a file, `importArchive` into boards). Every file goes this way, small
 * ones in one part; the server says how large a part is.
 *
 * A part that gets no answer, or the server's own failure, is sent again a
 * few times after a pause. Giving the upload up (the signal), or a failure,
 * throws away what the server has of it (`deleteUpload`); an upload whose
 * completion got no answer is left alone, since the file may have been made
 * (the server lets an upload end by itself in a day, and a file nothing
 * names is collected).
 *
 * A tab closed in the middle leaves its upload open on the server until it
 * ends: it counts toward the reader's open uploads until then.
 */
import { BackendError, type UploadedFile, type UploadOptions } from '../state/backend.ts';

import type { Schemas } from './convert.ts';
import { underWorkspace } from './requests.ts';
import type { Http } from './http.ts';

/** How many times one part is sent before the upload fails. */
const PART_TRIES = 3;
/** The pause before a part is sent again, in milliseconds, times the tries so far. */
const PART_PAUSE = 500;

const pause = (milliseconds: number, signal: AbortSignal | undefined): Promise<void> =>
  new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, milliseconds);
    signal?.addEventListener(
      'abort',
      () => {
        clearTimeout(timer);
        reject(signal.reason as Error);
      },
      { once: true },
    );
  });

/** The `Content-Digest` of some bytes: their SHA-256, as RFC 9530 writes it. */
export async function contentDigest(bytes: Blob): Promise<string> {
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', await bytes.arrayBuffer()));
  let binary = '';
  for (const byte of hash) binary += String.fromCharCode(byte);
  return `sha-256=:${btoa(binary)}:`;
}

/** Whether a failed part is worth sending again: no answer, or the server failed. */
const worthAgain = (error: unknown): boolean =>
  error instanceof BackendError && (error.status === 0 || error.status === 500);

/**
 * Sends `file` in parts to an upload that `open` opens, and ends it with
 * `finish`. What the two are is the caller's: a file of a board, or an
 * archive to import.
 */
export async function sendInParts<T>(
  http: Http,
  workspaceId: string,
  file: File,
  options: UploadOptions,
  open: () => Promise<Schemas['UploadOpened']>,
  finish: (uploadId: string, signal: AbortSignal | undefined) => Promise<T>,
): Promise<T> {
  const { signal, onProgress } = options;
  signal?.throwIfAborted();
  // Not given up halfway: once the server has opened it, its identifier is
  // needed to throw it away.
  const { uploadId, partSize, partCount } = await open();
  let finishing = false;
  try {
    let sent = 0;
    for (let number = 1; number <= partCount; number += 1) {
      const part = file.slice((number - 1) * partSize, Math.min(file.size, number * partSize));
      const digest = await contentDigest(part);
      for (let tries = 1; ; tries += 1) {
        signal?.throwIfAborted();
        try {
          await http.request(
            underWorkspace('/uploads/{uploadId}/parts/{partNumber}'),
            { workspaceId, uploadId, partNumber: String(number) },
            { method: 'PUT', bytes: part, headers: { 'Content-Digest': digest }, signal },
          );
          break;
        } catch (error) {
          if (tries >= PART_TRIES || !worthAgain(error)) throw error;
          await pause(PART_PAUSE * tries, signal);
        }
      }
      sent += part.size;
      onProgress?.({ sentBytes: sent, totalBytes: file.size });
    }
    signal?.throwIfAborted();
    finishing = true;
    const done = await finish(uploadId, signal);
    if (partCount === 0) onProgress?.({ sentBytes: 0, totalBytes: 0 });
    return done;
  } catch (error) {
    // No answer to the end: it may have happened; the upload is left to end.
    if (finishing && error instanceof BackendError && error.status === 0) throw error;
    // Given up or failed: what the server holds of it is thrown away (an
    // upload that has ended already is 404, which is no matter).
    void http
      .request(
        underWorkspace('/uploads/{uploadId}'),
        { workspaceId, uploadId },
        { method: 'DELETE' },
      )
      .catch(() => undefined);
    throw error;
  }
}

/** The upload's request for a file. */
function newUploadOf(file: File): Schemas['NewUpload'] {
  return { name: file.name, size: file.size, ...(file.type ? { contentType: file.type } : {}) };
}

/** A file of a board: opened by `createUpload`, ended into a file by `completeUpload`. */
export function uploadInParts(
  http: Http,
  { workspaceId, boardId }: { workspaceId: string; boardId: string },
  file: File,
  options: UploadOptions = {},
): Promise<UploadedFile> {
  return sendInParts(
    http,
    workspaceId,
    file,
    options,
    () =>
      http.request<Schemas['UploadOpened']>(
        underWorkspace('/boards/{boardId}/uploads'),
        { workspaceId, boardId },
        { method: 'POST', json: newUploadOf(file) },
      ),
    async (uploadId, signal) => {
      const done = await http.request<Schemas['UploadedFile']>(
        underWorkspace('/uploads/{uploadId}/complete'),
        { workspaceId, uploadId },
        { method: 'POST', signal },
      );
      return {
        fileId: done.fileId,
        name: file.name,
        size: file.size,
        contentType: file.type || 'application/octet-stream',
      };
    },
  );
}

/**
 * An archive to import into a workspace: opened by `createImportUpload`, ended
 * by `importArchive` (in place of `completeUpload`), which says what it left out.
 */
export function importInParts(
  http: Http,
  workspaceId: string,
  file: File,
  options: UploadOptions = {},
): Promise<Schemas['ImportResult']> {
  return sendInParts(
    http,
    workspaceId,
    file,
    options,
    () =>
      http.request<Schemas['UploadOpened']>(
        underWorkspace('/archive/uploads'),
        { workspaceId },
        { method: 'POST', json: newUploadOf(file) },
      ),
    async (uploadId, signal) => {
      const request: Schemas['ImportRequest'] = { uploadId };
      return http.request<Schemas['ImportResult']>(
        underWorkspace('/archive/import'),
        { workspaceId },
        { method: 'POST', json: request, signal },
      );
    },
  );
}
