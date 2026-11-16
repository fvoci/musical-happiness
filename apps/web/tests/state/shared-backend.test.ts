/**
 * The backend of a reader by a board's public link, against a stand-in for
 * `fetch`: every reading goes to `/api/v1/shared/{containerId}` with the read
 * token and without the session's cookie, and nothing is ever written.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { SHARED_PLACE, SharedBackend } from '$lib/api/shared-backend.ts';
import { isBackendError } from '$lib/state/index.ts';

interface Sent {
  url: string;
  method: string;
  credentials: RequestCredentials | undefined;
}

/** Answers every request with `answer`, and keeps what was sent. */
function serve(answer: unknown): Sent[] {
  const sent: Sent[] = [];
  vi.stubGlobal('fetch', (url: string, init: RequestInit = {}) => {
    sent.push({ url, method: init.method ?? 'GET', credentials: init.credentials });
    return Promise.resolve(
      new Response(JSON.stringify(answer), { headers: { 'Content-Type': 'application/json' } }),
    );
  });
  return sent;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

const BOARD = {
  id: 'b1',
  workspaceId: '',
  title: 'Shared',
  type: 'P',
  cardProperties: [],
  properties: {},
  createAt: 1,
  updateAt: 1,
  deleteAt: 0,
};

describe('the backend of a public link', () => {
  it('reads the board by the token alone, in a place of its own, without the cookie', async () => {
    const sent = serve(BOARD);
    const backend = new SharedBackend('/base', 'b1', 'to/ken');
    const board = await backend.board('b1');
    expect(board.workspaceId).toBe(SHARED_PLACE);
    expect(sent).toEqual([
      { url: '/base/api/v1/shared/b1?readToken=to%2Fken', method: 'GET', credentials: 'omit' },
    ]);
    expect(backend.fileUrl('b1', 'f1')).toBe('/base/api/v1/shared/b1/files/f1?readToken=to%2Fken');
  });

  it('reads the views, the comments of a card and its files under the board', async () => {
    const sent = serve([]);
    const backend = new SharedBackend('', 'b1', 't');
    await backend.views('b1');
    await backend.comments('b1', 'c1');
    await backend.attachments('b1', 'c1');
    expect(sent.map((one) => one.url)).toEqual([
      '/api/v1/shared/b1/blocks?type=view&readToken=t',
      '/api/v1/shared/b1/blocks?parentId=c1&type=comment&readToken=t',
      '/api/v1/shared/b1/blocks?parentId=c1&type=attachment&readToken=t',
    ]);
  });

  it('has only the link’s board, and nobody in it', async () => {
    const sent = serve([]);
    const backend = new SharedBackend('', 'b1', 't');
    const other = await backend.board('b2').catch((error: unknown) => error);
    expect(isBackendError(other) && other.status).toBe(404);
    expect(backend.fileUrl('b2', 'f1')).toBeUndefined();
    expect(await backend.members('b1')).toEqual([]);
    expect(await backend.usersById(SHARED_PLACE, ['u1'])).toEqual([]);
    expect((await backend.me()).id).toBe('');
    expect(sent).toEqual([]);
  });

  it('writes nothing: every change is refused before a request is made', async () => {
    const sent = serve({});
    const backend = new SharedBackend('', 'b1', 't');
    const refusals = await Promise.all([
      backend.patchCard('b1', 'c1', { title: 'x' }).catch((error: unknown) => error),
      backend.createComment('b1', 'c1', { type: 'doc', content: [] }).catch((e: unknown) => e),
      backend.deleteBlock('b1', 'c1').catch((error: unknown) => error),
      backend.patchView('b1', 'v1', {}).catch((error: unknown) => error),
      backend.joinBoard('b1').catch((error: unknown) => error),
    ]);
    for (const refusal of refusals) expect(isBackendError(refusal) && refusal.status).toBe(403);
    expect(sent).toEqual([]);
  });
});

describe('the backend of a wiki’s public link', () => {
  it('reads the tree and a page under the wiki by the token, and refuses every change', async () => {
    const sent = serve([]);
    const backend = new SharedBackend('', 'w1', 't');
    const wikis = backend.wiki;
    expect(await wikis.pages('w1')).toEqual([]);
    sent.length = 0;
    serve({
      id: 'p1',
      wikiId: 'w1',
      parentId: '',
      title: 'Planting',
      icon: '',
      order: 'a',
      body: null,
      createdBy: '',
      modifiedBy: '',
      createAt: 1,
      updateAt: 1,
    });
    const page = await wikis.page('w1', 'p1');
    expect([page.title, page.createdBy]).toEqual(['Planting', '']);
    await expect(wikis.pages('w2')).rejects.toSatisfy(isBackendError);
    for (const write of [
      () => wikis.createPage('w1', { title: 'x' }),
      () => wikis.patchPage('w1', 'p1', { title: 'y' }),
      () => wikis.deletePage('w1', 'p1'),
    ]) {
      await expect(write()).rejects.toSatisfy(isBackendError);
    }
  });

  it('reads no comments or versions of a page, asking the server nothing, and writes none', async () => {
    const sent = serve([]);
    const wikis = new SharedBackend('', 'w1', 't').wiki;
    expect([
      await wikis.comments('w1', 'p1'),
      await wikis.commentVersions('w1', 'p1', 'c1'),
      await wikis.bodyVersions.list('w1', 'p1'),
    ]).toEqual([[], [], []]);
    expect(sent).toEqual([]);
    for (const write of [
      () => wikis.createComment('w1', 'p1', { type: 'doc', content: [] }),
      () => wikis.deleteComment('w1', 'p1', 'c1'),
      () => wikis.restoreComment('w1', 'p1', 'c1'),
    ]) {
      await expect(write()).rejects.toSatisfy(isBackendError);
    }
  });

  it('asks the server at the wiki’s addresses of the link', async () => {
    const sent = serve([]);
    const backend = new SharedBackend('', 'w1', 'to/ken');
    await backend.wiki.pages('w1');
    expect(sent.map((one) => [one.url, one.credentials])).toEqual([
      ['/api/v1/shared/w1/pages?readToken=to%2Fken', 'omit'],
    ]);
  });
});
