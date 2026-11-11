/**
 * Uploading a file to the server in parts, against a stand-in for `fetch`:
 * what is sent for each step, the digest of each part, progress, a part sent
 * again, giving up, and the limit read from the server's settings.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
import { contentDigest } from '$lib/api/upload.ts';
import { isBackendError } from '$lib/state/index.ts';

interface Call {
  method: string;
  url: string;
  size: number | undefined;
  headers: Record<string, string>;
  json: unknown;
}

/** A server that takes uploads in parts of `partSize` bytes; `fail` decides a part's answer. */
function uploadServer(options: {
  partSize: number;
  maxFileSize?: number;
  fail?: (number: number, attempt: number) => number | undefined;
}): Call[] {
  const calls: Call[] = [];
  const attempts = new Map<number, number>();
  vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
    const method = init.method ?? 'GET';
    init.signal?.throwIfAborted();
    calls.push({
      method,
      url,
      size: init.body instanceof Blob ? init.body.size : undefined,
      headers: init.headers as Record<string, string>,
      json: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
    });
    const answer = (status: number, body?: unknown): Response =>
      new Response(body === undefined ? null : JSON.stringify(body), { status });
    if (url.endsWith('/client-config')) {
      return answer(200, { maxFileSize: options.maxFileSize ?? 1000 });
    }
    if (method === 'POST' && url.endsWith('/uploads')) {
      const size = (calls.at(-1)?.json as { size: number }).size;
      return answer(201, {
        uploadId: 'up1',
        partSize: options.partSize,
        partCount: Math.ceil(size / options.partSize),
        expiresAt: 1,
      });
    }
    const part = /\/parts\/(\d+)$/.exec(url);
    if (method === 'PUT' && part) {
      const number = Number(part[1]);
      const attempt = (attempts.get(number) ?? 0) + 1;
      attempts.set(number, attempt);
      const status = options.fail?.(number, attempt);
      if (status === 0) throw new TypeError('no answer');
      return status ? answer(status, { error: 'internal', message: 'x' }) : answer(204);
    }
    if (url.endsWith('/complete')) return answer(201, { fileId: 'f1.txt' });
    if (method === 'DELETE') return answer(204);
    return answer(404, { error: 'not_found', message: url });
  });
  return calls;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('uploading a file to the server', () => {
  it('opens an upload, sends each part with its digest, tells the progress, and completes', async () => {
    const calls = uploadServer({ partSize: 4 });
    const file = new File(['0123456789'], 'notes.txt', { type: 'text/plain' });
    const progress: number[] = [];
    const uploaded = await new ServerBackend('/base').upload('b1', file, {
      onProgress: ({ sentBytes }) => progress.push(sentBytes),
    });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      'GET /base/api/v1/client-config',
      'POST /base/api/v1/boards/b1/uploads',
      'PUT /base/api/v1/uploads/up1/parts/1',
      'PUT /base/api/v1/uploads/up1/parts/2',
      'PUT /base/api/v1/uploads/up1/parts/3',
      'POST /base/api/v1/uploads/up1/complete',
    ]);
    expect(calls[1]?.json).toEqual({ name: 'notes.txt', size: 10, contentType: 'text/plain' });
    expect(calls.slice(2, 5).map((call) => call.size)).toEqual([4, 4, 2]);
    expect(calls[2]?.headers['Content-Digest']).toBe(await contentDigest(new Blob(['0123'])));
    expect(calls[2]?.headers['Content-Type']).toBe('application/octet-stream');
    expect(progress).toEqual([4, 8, 10]);
    expect(uploaded).toEqual({
      fileId: 'f1.txt',
      name: 'notes.txt',
      size: 10,
      contentType: 'text/plain',
    });
  });

  it('writes the digest as RFC 9530 does', async () => {
    // The SHA-256 of "abc".
    expect(await contentDigest(new Blob(['abc']))).toBe(
      'sha-256=:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:',
    );
  });

  it('completes an empty file without parts', async () => {
    const calls = uploadServer({ partSize: 4 });
    await new ServerBackend('').upload('b1', new File([], 'empty.txt'));
    expect(calls.map((call) => call.method)).toEqual(['GET', 'POST', 'POST']);
  });

  it('sends a part again when it got no answer or the server failed, and gives up after three', async () => {
    const calls = uploadServer({
      partSize: 4,
      fail: (number, attempt) =>
        number === 2 && attempt === 1 ? 0 : number === 2 && attempt === 2 ? 500 : undefined,
    });
    await new ServerBackend('').upload('b1', new File(['0123456789'], 'a.txt'));
    expect(calls.filter((call) => call.url.endsWith('/parts/2'))).toHaveLength(3);

    const failing = uploadServer({ partSize: 4, fail: () => 500 });
    const error = await new ServerBackend('')
      .upload('b1', new File(['0123456789'], 'a.txt'))
      .catch((caught: unknown) => caught);
    expect(isBackendError(error) && error.status).toBe(500);
    expect(failing.filter((call) => call.method === 'PUT')).toHaveLength(3);
    // What the server held of it is thrown away.
    expect(failing.at(-1)).toMatchObject({ method: 'DELETE', url: '/api/v1/uploads/up1' });
  });

  it('does not send a part again that the server refused', async () => {
    const calls = uploadServer({ partSize: 4, fail: () => 400 });
    const error = await new ServerBackend('')
      .upload('b1', new File(['0123456789'], 'a.txt'))
      .catch((caught: unknown) => caught);
    expect(isBackendError(error) && error.status).toBe(400);
    expect(calls.filter((call) => call.method === 'PUT')).toHaveLength(1);
  });

  it('throws the upload away when it is given up', async () => {
    const calls = uploadServer({ partSize: 4 });
    const controller = new AbortController();
    const uploading = new ServerBackend('').upload('b1', new File(['0123456789'], 'a.txt'), {
      signal: controller.signal,
      onProgress: () => {
        controller.abort();
      },
    });
    await expect(uploading).rejects.toThrow();
    expect(calls.some((call) => call.url.endsWith('/complete'))).toBe(false);
    expect(calls.at(-1)).toMatchObject({ method: 'DELETE', url: '/api/v1/uploads/up1' });
  });

  it('refuses a file over the server’s limit before sending anything, with the limit', async () => {
    const calls = uploadServer({ partSize: 4, maxFileSize: 5 });
    const error = await new ServerBackend('')
      .upload('b1', new File(['0123456789'], 'big.bin'))
      .catch((caught: unknown) => caught);
    expect(isBackendError(error) && [error.status, error.limit]).toEqual([413, 5]);
    expect(calls.map((call) => call.method)).toEqual(['GET']);
  });

  it('attaches an uploaded file as a block under the card that names it', async () => {
    const calls: Call[] = [];
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      const json = JSON.parse(init.body as string) as { title: string; fields: object }[];
      calls.push({ method: init.method ?? 'GET', url, size: undefined, headers: {}, json });
      const [block] = json;
      return Promise.resolve(
        new Response(
          JSON.stringify([
            {
              ...block,
              id: 'a1',
              body: null,
              createdBy: 'u',
              modifiedBy: 'u',
              schema: 1,
              deleteAt: 0,
            },
          ]),
          { status: 201 },
        ),
      );
    });
    const attachment = await new ServerBackend('').createAttachment('b1', 'c1', {
      fileId: 'f1.txt',
      name: 'notes.txt',
      size: 10,
      contentType: 'text/plain',
    });
    expect(calls[0]).toMatchObject({
      method: 'POST',
      url: '/api/v1/boards/b1/blocks',
      json: [
        {
          boardId: 'b1',
          parentId: 'c1',
          type: 'attachment',
          title: 'notes.txt',
          fields: { fileId: 'f1.txt', size: 10, contentType: 'text/plain' },
        },
      ],
    });
    expect(attachment).toMatchObject({ id: 'a1', fileId: 'f1.txt', name: 'notes.txt', size: 10 });
    expect(new ServerBackend('/base').fileUrl('b1', 'f1.txt')).toBe(
      '/base/api/v1/boards/b1/files/f1.txt',
    );
  });

  it('reads a file’s bytes from its board', async () => {
    const urls: string[] = [];
    vi.stubGlobal('fetch', (url: string) => {
      urls.push(url);
      return Promise.resolve(new Response('bytes', { status: 200 }));
    });
    const bytes = await new ServerBackend('/base').fileBytes('b1', 'f1.txt');
    expect(await bytes.text()).toBe('bytes');
    expect(urls).toEqual(['/base/api/v1/boards/b1/files/f1.txt']);
  });
});
