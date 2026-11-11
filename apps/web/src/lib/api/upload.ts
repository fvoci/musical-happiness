/**
 * Uploading a file to the server in parts (`docs/contract-pending.md`, 4):
 * an upload is opened with the file's name and size (`createUpload`), each
 * part is sent with the SHA-256 of its bytes (`putUploadPart`), and the
 * upload is completed into a file (`completeUpload`). Every file goes this
 * way, small ones in one part; the server says how large a part is.
 *
 * A part that gets no answer, or the server's own failure, is sent again a
 * few times. Giving the upload up (the signal) throws away what the server
 * has of it (`deleteUpload`).
 */
import { BackendError, type UploadedFile, type UploadOptions } from '../state/backend.ts';

import type { Schemas } from './convert.ts';
import type { Http } from './http.ts';

/** How many times one part is sent before the upload fails. */
const PART_TRIES = 3;

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

export async function uploadInParts(
  http: Http,
  boardId: string,
  file: File,
  options: UploadOptions = {},
): Promise<UploadedFile> {
  const { signal, onProgress } = options;
  signal?.throwIfAborted();
  const request: Schemas['NewUpload'] = {
    name: file.name,
    size: file.size,
    ...(file.type ? { contentType: file.type } : {}),
  };
  const opened = await http.request<Schemas['UploadOpened']>(
    '/api/v1/boards/{boardId}/uploads',
    { boardId },
    { method: 'POST', json: request, signal },
  );
  const { uploadId, partSize, partCount } = opened;
  try {
    let sent = 0;
    for (let number = 1; number <= partCount; number += 1) {
      const part = file.slice((number - 1) * partSize, Math.min(file.size, number * partSize));
      const digest = await contentDigest(part);
      for (let tries = 1; ; tries += 1) {
        signal?.throwIfAborted();
        try {
          await http.request(
            '/api/v1/uploads/{uploadId}/parts/{partNumber}',
            { uploadId, partNumber: String(number) },
            { method: 'PUT', bytes: part, headers: { 'Content-Digest': digest }, signal },
          );
          break;
        } catch (error) {
          if (tries >= PART_TRIES || !worthAgain(error)) throw error;
        }
      }
      sent += part.size;
      onProgress?.({ sentBytes: sent, totalBytes: file.size });
    }
    signal?.throwIfAborted();
    const done = await http.request<Schemas['UploadedFile']>(
      '/api/v1/uploads/{uploadId}/complete',
      { uploadId },
      { method: 'POST', signal },
    );
    if (partCount === 0) onProgress?.({ sentBytes: 0, totalBytes: 0 });
    return {
      fileId: done.fileId,
      name: file.name,
      size: file.size,
      contentType: file.type || 'application/octet-stream',
    };
  } catch (error) {
    // Given up or failed: what the server holds of it is thrown away (it ends by itself in a day otherwise).
    void http
      .request('/api/v1/uploads/{uploadId}', { uploadId }, { method: 'DELETE' })
      .catch(() => undefined);
    throw error;
  }
}
