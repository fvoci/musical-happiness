/**
 * Downloading what the server offers as a file (an archive), with its
 * progress: the answer's body is read as it arrives and counted against its
 * length.
 *
 * An answer that is no success is not read here: the request is made once
 * more through `Http.bytes`, whose reading of errors is the one of every
 * request (`http.ts`, which this file may not change). That second request
 * cannot be given up, and a refusal the server counts (429) is counted
 * twice; once errors are read apart from requests, this asks once.
 */
import { BackendError, type DownloadedFile, type DownloadOptions } from '../state/backend.ts';

import type { ApiPath, Http } from './http.ts';

/** The file name of a `Content-Disposition`, or undefined. */
export function dispositionName(header: string | null): string | undefined {
  if (!header) return undefined;
  const encoded = /filename\*\s*=\s*UTF-8''([^;]+)/i.exec(header)?.[1];
  if (encoded) {
    try {
      return decodeURIComponent(encoded.trim());
    } catch {
      // Not percent-encoded after all: the plain name, if there is one.
    }
  }
  const plain = /filename\s*=\s*("([^"]*)"|[^;]+)/i.exec(header);
  return (plain?.[2] ?? plain?.[1])?.trim() || undefined;
}

export async function downloadWithProgress(
  http: Http,
  path: ApiPath,
  params: Record<string, string>,
  fallbackName: string,
  options: DownloadOptions = {},
): Promise<DownloadedFile> {
  const { signal, onProgress } = options;
  let response: Response;
  try {
    response = await fetch(http.url(path, params), { credentials: 'same-origin', signal });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new BackendError(0, { message: `GET ${path}: no answer` });
  }
  const name = dispositionName(response.headers.get('Content-Disposition')) ?? fallbackName;
  if (!response.ok) {
    void response.body?.cancel();
    // Throws the server's refusal, read as every other one.
    return { bytes: await http.bytes(path, params), name };
  }
  const total = Number(response.headers.get('Content-Length') ?? 0) || 0;
  const body = response.body;
  if (!body) return { bytes: await response.blob(), name };
  // Counted on the way through; the browser keeps the bytes (a Blob), not this page.
  let received = 0;
  onProgress?.({ sentBytes: 0, totalBytes: total });
  const counted = body.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      transform(chunk, controller) {
        received += chunk.byteLength;
        onProgress?.({ sentBytes: received, totalBytes: Math.max(total, received) });
        controller.enqueue(chunk);
      },
    }),
  );
  let bytes: Blob;
  try {
    bytes = await new Response(counted, {
      headers: {
        'Content-Type': response.headers.get('Content-Type') ?? 'application/octet-stream',
      },
    }).blob();
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new BackendError(0, { message: `GET ${path}: the answer broke off` });
  }
  // A body that ends early without breaking (a proxy that cut it) is no archive either.
  if (total > 0 && received !== total) {
    throw new BackendError(0, { message: `GET ${path}: ${received} of ${total} bytes arrived` });
  }
  return { bytes, name };
}
