/**
 * The backend that calls the server, against a stand-in for `fetch`: what
 * it sends, and how the server's shapes become the screens'. What the real
 * server does with it is the work of `e2e-live/`.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { serverBackend } from './support.ts';
import { cardOfBlock } from '$lib/api/convert.ts';
import { isBackendError } from '$lib/state/index.ts';

interface Sent {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
}

/** Answers every request with `answer`, and keeps what was sent. */
function serve(answer: unknown, status = 200): Sent[] {
  const sent: Sent[] = [];
  vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
    sent.push({
      url,
      method: init.method ?? 'GET',
      headers: init.headers as Record<string, string>,
      body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
    });
    return Promise.resolve(
      new Response(status === 204 ? null : JSON.stringify(answer), {
        status,
        // An error is the server's problem details (RFC 9457).
        headers: {
          'Content-Type': status >= 400 ? 'application/problem+json' : 'application/json',
        },
      }),
    );
  });
  return sent;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

const BLOCK = {
  boardId: 'b',
  body: null,
  parentId: 'c',
  createAt: 1,
  updateAt: 2,
  deleteAt: 0,
  createdBy: 'u',
  modifiedBy: 'u',
  schema: 1,
  fields: {},
};

describe('the server backend', () => {
  it('signs in with the text as the name and as the email address, and says it is a script', async () => {
    const sent = serve(undefined, 204);
    await serverBackend('/base').signIn('ann@home', 'secret pass');
    expect(sent).toHaveLength(1);
    expect(sent[0]?.url).toBe('/base/api/v1/login');
    expect(sent[0]?.body).toEqual({
      username: 'ann@home',
      email: 'ann@home',
      password: 'secret pass',
    });
    expect(sent[0]?.headers['X-Requested-With']).toBe('XMLHttpRequest');
  });

  it('asks for a board read before under its own workspace, also after entering another', async () => {
    const sent = serve({ ...BLOCK, id: 'b9', workspaceId: 'w2', title: 'Elsewhere', type: 'O' });
    const backend = serverBackend('');
    await backend.board('b9');
    backend.enter('w3');
    await backend.views('b9').catch(() => undefined);
    await backend.patchCard('b9', 'c9', { title: 'x' }).catch(() => undefined);
    expect(sent.map((one) => one.url)).toEqual([
      '/api/v1/w/w1/boards/b9',
      '/api/v1/w/w2/boards/b9/blocks?type=view',
      '/api/v1/w/w2/cards/c9',
    ]);
  });

  it('asks once for the people by their identifiers, each once, and not for none', async () => {
    const sent = serve([
      { id: 'u1', username: 'ann', email: '', createAt: 1, updateAt: 1, deleteAt: 0 },
    ]);
    const backend = serverBackend('');
    expect(await backend.usersById('w1', [])).toEqual([]);
    const found = await backend.usersById('w1', ['u1', 'u2', 'u1']);
    expect(found.map((user) => user.id)).toEqual(['u1']);
    expect(sent.map((one) => [one.method, one.url, one.body])).toEqual([
      ['POST', '/api/v1/w/w1/users', ['u1', 'u2']],
    ]);
  });

  it('asks for a long list of people in parts the server takes', async () => {
    const sent = serve([]);
    const ids = Array.from({ length: 450 }, (_, at) => `u${at}`);
    await serverBackend('').usersById('w1', [...ids, ...ids]);
    expect(sent.map((one) => (one.body as string[]).length)).toEqual([200, 200, 50]);
    expect(sent.flatMap((one) => one.body as string[])).toEqual(ids);
  });

  it('makes a card from a draft, its body along', async () => {
    const sent = serve({
      ...BLOCK,
      id: 'c1',
      title: 'New',
      icon: '',
      isTemplate: false,
      properties: { status: 'todo' },
      hasBody: false,
      tasksTotal: 0,
      tasksDone: 0,
    });
    const backend = serverBackend('');
    const card = await backend.createCard('b', { properties: { status: 'todo' } });
    expect(sent[0]).toMatchObject({
      url: '/api/v1/w/w1/boards/b/cards',
      method: 'POST',
      body: { title: '', icon: '', isTemplate: false, properties: { status: 'todo' } },
    });
    expect(card).toMatchObject({
      id: 'c1',
      type: 'card',
      fields: { properties: { status: 'todo' } },
    });

    const body = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x' }] }],
    };
    await backend.createCard('b', { title: 'Copy', body });
    expect(sent[1]?.body).toMatchObject({ title: 'Copy', body });
  });

  it('makes a comment as a block under the card, its text the body', async () => {
    const body = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hi' }] }],
    };
    const sent = serve([{ ...BLOCK, id: 'm', type: 'comment', title: '', body }], 201);
    const comment = await serverBackend('').createComment('b', 'c', body);
    expect(sent[0]).toMatchObject({
      url: '/api/v1/w/w1/boards/b/blocks',
      method: 'POST',
      body: [{ boardId: 'b', parentId: 'c', type: 'comment', body }],
    });
    expect(comment).toMatchObject({ id: 'm', type: 'comment', body });
  });

  it('changes a comment by its block with the version it was read at, and reads it again', async () => {
    const body = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'New' }] }],
    };
    const comment = { ...BLOCK, id: 'm', type: 'comment', title: '', updateAt: 9, body };
    const sent: Sent[] = [];
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      sent.push({
        url,
        method: init.method ?? 'GET',
        headers: {},
        body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
      });
      return Promise.resolve(
        init.method === 'PATCH'
          ? new Response(null, { status: 204 })
          : new Response(JSON.stringify([comment]), { status: 200 }),
      );
    });
    const changed = await serverBackend('').patchComment('b', 'm', { body, readAt: 7 });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'GET /api/v1/w/w1/boards/b/blocks?blockId=m',
      'PATCH /api/v1/w/w1/boards/b/blocks/m',
      'GET /api/v1/w/w1/boards/b/blocks?blockId=m',
    ]);
    expect(sent[1]?.body).toEqual({ body, readAt: 7 });
    expect(changed).toMatchObject({ id: 'm', body, updateAt: 9 });

    // No text is no comment: refused before anything is sent.
    const none = serve([]);
    const empty = await serverBackend('')
      .patchComment('b', 'm', { body: null, readAt: 1 })
      .catch((error: unknown) => error);
    expect(isBackendError(empty) && empty.status).toBe(400);
    expect(none).toHaveLength(0);

    // A block that is not a comment is not changed.
    const other = serve([{ ...BLOCK, id: 'c', type: 'card', title: 'x' }]);
    const refused = await serverBackend('')
      .patchComment('b', 'c', { body, readAt: 1 })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(404);
    expect(other.map((one) => one.method)).toEqual(['GET']);
  });

  it('reads the earlier texts of a comment, and says when and by whom it was changed', async () => {
    const body = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Before' }] }],
    };
    const sent = serve([
      { version: 1, body, writtenBy: 'u1', writtenAt: 5 },
      { version: 3, body: null, writtenBy: 'u2', writtenAt: 8 },
    ]);
    expect(await serverBackend('').commentVersions('b', 'm')).toEqual([
      { version: 1, body, writtenBy: 'u1', writtenAt: 5 },
      { version: 3, body: null, writtenBy: 'u2', writtenAt: 8 },
    ]);
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'GET /api/v1/w/w1/boards/b/blocks/m/versions',
    ]);

    serve([{ ...BLOCK, id: 'n', type: 'comment', title: '', body, editedAt: 9, modifiedBy: 'u2' }]);
    const [comment] = await serverBackend('').comments('b', 'c');
    expect(comment).toMatchObject({ editedAt: 9, modifiedBy: 'u2' });
  });

  it("reads a card's comments and attachments as blocks under it", async () => {
    const body = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Body' }] }],
    };
    serve([{ ...BLOCK, id: 'n', type: 'comment', title: '', body }]);
    expect((await serverBackend('').comments('b', 'c'))[0]?.body).toEqual(body);
    // One stored before comments had bodies: its title, a paragraph a line.
    const sent = serve([{ ...BLOCK, id: 'm', type: 'comment', title: 'Looks good\nShip it' }]);
    const backend = serverBackend('');
    const [comment] = await backend.comments('b', 'c');
    expect(sent[0]?.url).toBe('/api/v1/w/w1/boards/b/blocks?parentId=c&type=comment');
    expect(comment?.body).toEqual({
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: 'Looks good' }] },
        { type: 'paragraph', content: [{ type: 'text', text: 'Ship it' }] },
      ],
    });

    serve([
      {
        ...BLOCK,
        id: 'a',
        type: 'attachment',
        title: 'plan.pdf',
        fields: { fileId: 'f1', size: 120, contentType: 'application/pdf' },
      },
    ]);
    expect(await backend.attachments('b', 'c')).toMatchObject([
      { id: 'a', fileId: 'f1', name: 'plan.pdf', size: 120, contentType: 'application/pdf' },
    ]);
    // An older block names its file `attachmentId`, and says no size.
    serve([
      { ...BLOCK, id: 'o', type: 'attachment', title: 'old.png', fields: { attachmentId: 'f0' } },
    ]);
    expect(await backend.attachments('b', 'c')).toMatchObject([
      { id: 'o', fileId: 'f0', name: 'old.png', size: 0, contentType: '' },
    ]);
  });
});

describe('changing a card on the server', () => {
  const CARD = {
    ...BLOCK,
    id: 'c1',
    parentId: 'b',
    title: 'Card',
    icon: '',
    isTemplate: false,
    body: null,
    hasBody: false,
    tasksTotal: 0,
    tasksDone: 0,
  };

  /**
   * A server of one card whose values are `values`. It keeps what it is
   * sent as the server does: the values set are merged in, the ones named
   * cleared.
   */
  function oneCard(values: Record<string, unknown>): {
    sent: Sent[];
    card: () => typeof CARD & { properties: Record<string, unknown> };
  } {
    let card = { ...CARD, properties: values };
    const sent: Sent[] = [];
    vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
      const body =
        typeof init.body === 'string'
          ? (JSON.parse(init.body) as Record<string, unknown>)
          : undefined;
      sent.push({
        url,
        method: init.method ?? 'GET',
        headers: init.headers as Record<string, string>,
        body,
      });
      // A little wait, as a network has: two changes could pass each other.
      await new Promise((resolve) => setTimeout(resolve, 5));
      if (init.method === 'PATCH' && url.endsWith('/cards/c1')) {
        const patch = body as {
          title?: string;
          updatedProperties?: Record<string, unknown>;
          deletedProperties?: string[];
        };
        const properties = { ...card.properties, ...patch.updatedProperties };
        for (const id of patch.deletedProperties ?? []) {
          // eslint-disable-next-line @typescript-eslint/no-dynamic-delete -- the patch names them
          delete properties[id];
        }
        card = { ...card, title: patch.title ?? card.title, properties };
      }
      return new Response(JSON.stringify(card), { status: 200 });
    });
    return { sent, card: () => card };
  }

  it('sends only the title when no value changes', async () => {
    const { sent } = oneCard({ a: '1' });
    await serverBackend('').patchCard('b', 'c1', { title: 'New' });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual(['PATCH /api/v1/w/w1/cards/c1']);
    expect(sent[0]?.body).toEqual({ title: 'New' });
  });

  it('sends the values it sets and the ones it clears, and nothing else, without reading the card', async () => {
    const { sent, card } = oneCard({ a: '1', b: '2', c: '3' });
    const answer = await serverBackend('').patchCard('b', 'c1', {
      updatedProperties: { b: '4' },
      deletedProperties: ['a'],
    });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual(['PATCH /api/v1/w/w1/cards/c1']);
    expect(sent[0]?.body).toEqual({ updatedProperties: { b: '4' }, deletedProperties: ['a'] });
    expect(card().properties).toEqual({ b: '4', c: '3' });
    expect(answer.fields.properties).toEqual({ b: '4', c: '3' });
  });

  it('takes away the last value by the card patch, with a new title', async () => {
    const { sent, card } = oneCard({ a: '1' });
    await serverBackend('').patchCard('b', 'c1', { title: 'T', deletedProperties: ['a'] });
    expect(sent.map((one) => [one.method, one.url, one.body])).toEqual([
      ['PATCH', '/api/v1/w/w1/cards/c1', { title: 'T', deletedProperties: ['a'] }],
    ]);
    expect(card()).toMatchObject({ title: 'T', properties: {} });
  });

  it('sends two quick changes of one card one after the other, in the order they were made', async () => {
    const { sent, card } = oneCard({});
    const backend = serverBackend('');
    await Promise.all([
      backend.patchCard('b', 'c1', { updatedProperties: { a: '1' } }),
      backend.patchCard('b', 'c1', { updatedProperties: { a: '2' } }),
    ]);
    expect(sent.map((one) => one.body)).toEqual([
      { updatedProperties: { a: '1' } },
      { updatedProperties: { a: '2' } },
    ]);
    expect(card().properties).toEqual({ a: '2' });
  });

  it('reads the badges of a card from the server, and works them out from a block’s body', async () => {
    serve({
      ...CARD,
      properties: {},
      hasBody: true,
      tasksTotal: 3,
      tasksDone: 1,
      lastCommentAt: 44,
    });
    const answer = await serverBackend('').patchCard('b', 'c1', { title: 'x' });
    expect(answer).toMatchObject({ hasBody: true, tasksTotal: 3, tasksDone: 1, lastCommentAt: 44 });
    expect(answer).not.toHaveProperty('body');
    const block = cardOfBlock({
      ...BLOCK,
      id: 'c2',
      type: 'card',
      title: 'With tasks',
      fields: { properties: {} },
      body: {
        type: 'doc',
        content: [
          {
            type: 'taskList',
            content: [
              { type: 'taskItem', attrs: { checked: true }, content: [{ type: 'paragraph' }] },
              { type: 'taskItem', attrs: { checked: false }, content: [{ type: 'paragraph' }] },
            ],
          },
        ],
      },
    });
    expect(block).toMatchObject({ hasBody: true, tasksTotal: 2, tasksDone: 1 });
    expect(block).not.toHaveProperty('body');
    // A change message does not say when the last comment was posted.
    expect(block.lastCommentAt).toBeUndefined();
  });
});

describe('joining an open board on the server', () => {
  it('asks to join by the board and gives the membership', async () => {
    const sent = serve({
      boardId: 'b',
      userId: 'u',
      roles: '',
      schemeAdmin: false,
      schemeEditor: true,
      schemeCommenter: true,
      schemeViewer: true,
    });
    const member = await serverBackend('').joinBoard('b');
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'POST /api/v1/w/w1/boards/b/join',
    ]);
    expect(member).toMatchObject({ boardId: 'b', userId: 'u', schemeEditor: true });
  });
});

describe('the problems of the server', () => {
  it('reads a refusal as the problem details the server sends', async () => {
    serve(
      {
        type: '/problems/username-taken',
        title: 'User name taken',
        status: 409,
        detail: 'the user name is taken',
        instance: 'urn:uuid:r1',
        reason: 'username_taken',
        errors: [
          { pointer: '/username', detail: 'the user name is taken', reason: 'username_taken' },
          { parameter: 'page', detail: 'no number', reason: 'invalid' },
          { pointer: '/x' },
          'nothing',
        ],
        limit: 7,
        retryAfter: 3,
        requestId: 'r1',
      },
      409,
    );
    const refused = await serverBackend('')
      .me()
      .catch((error: unknown) => error);
    if (!isBackendError(refused)) throw new Error('not refused');
    expect(refused).toMatchObject({
      status: 409,
      type: '/problems/username-taken',
      reason: 'username_taken',
      message: 'the user name is taken',
      limit: 7,
      retryAfter: 3,
      requestId: 'r1',
    });
    // An entry that does not say why is no entry.
    expect(refused.errors).toEqual([
      { pointer: '/username', detail: 'the user name is taken', reason: 'username_taken' },
      { parameter: 'page', detail: 'no number', reason: 'invalid' },
    ]);
    expect(refused.refusalAt('/username')?.reason).toBe('username_taken');
    expect(refused.refusalAt('/email')).toBeUndefined();
  });

  it("goes by the status alone when an error answer is not the server's", async () => {
    // A proxy's page, or the old shape: nothing of it is read.
    vi.stubGlobal('fetch', () =>
      Promise.resolve(
        new Response(JSON.stringify({ error: 'conflict', message: 'x', reason: 'r', field: 'f' }), {
          status: 502,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    );
    const refused = await serverBackend('')
      .me()
      .catch((error: unknown) => error);
    if (!isBackendError(refused)) throw new Error('not refused');
    expect(refused).toMatchObject({ status: 500, type: 'about:blank', reason: undefined });
    expect(refused.errors).toEqual([]);
  });
});

describe('changing a view on the server', () => {
  it('patches its block without what the contract does not have, and reads it again', async () => {
    const sent = serve([
      {
        ...BLOCK,
        id: 'v',
        parentId: 'b',
        type: 'view',
        title: 'Board',
        fields: { cardOrder: ['c2', 'c1'] },
      },
    ]);
    const view = await serverBackend('').patchView('b', 'v', {
      updatedFields: { cardOrder: ['c2', 'c1'] },
      deleteAt: 0,
    });
    expect(sent.map((one) => [one.method, one.url, one.body])).toEqual([
      ['GET', '/api/v1/w/w1/boards/b/blocks?blockId=v', undefined],
      ['PATCH', '/api/v1/w/w1/boards/b/blocks/v', { updatedFields: { cardOrder: ['c2', 'c1'] } }],
      ['GET', '/api/v1/w/w1/boards/b/blocks?blockId=v', undefined],
    ]);
    expect(view.fields.cardOrder).toEqual(['c2', 'c1']);
  });

  const VIEW = {
    ...BLOCK,
    id: 'v',
    parentId: 'b',
    type: 'view',
    title: 'Board',
    fields: { cardOrder: ['c1', 'c2'] },
  };

  it('changes no block that is not a view', async () => {
    const sent = serve([{ ...VIEW, type: 'card' }]);
    const refused = await serverBackend('')
      .patchView('b', 'v', { updatedFields: { cardOrder: [] } })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(404);
    expect(sent.map((one) => one.method)).toEqual(['GET']);
  });

  it('keeps a change that was saved when only reading the view back fails', async () => {
    let calls = 0;
    vi.stubGlobal('fetch', (_url: string, init: RequestInit) => {
      calls += 1;
      if (calls === 3) return Promise.reject(new TypeError('offline'));
      if (init.method === 'PATCH') return Promise.resolve(new Response(null, { status: 204 }));
      return Promise.resolve(new Response(JSON.stringify([VIEW]), { status: 200 }));
    });
    const view = await serverBackend('').patchView('b', 'v', {
      updatedFields: { cardOrder: ['c2', 'c1'] },
    });
    expect(view.fields.cardOrder).toEqual(['c2', 'c1']);
  });

  it('makes a view as a block of the board, and refuses an answer without one', async () => {
    const sent = serve([{ ...VIEW, fields: { viewType: 'calendar' } }], 201);
    const view = await serverBackend('').createView('b', {
      title: 'Calendar view',
      fields: { viewType: 'calendar', dateDisplayPropertyId: 'due' } as never,
    });
    expect(sent.map((one) => [one.method, one.url])).toEqual([
      ['POST', '/api/v1/w/w1/boards/b/blocks'],
    ]);
    expect(sent[0]?.body).toEqual([
      {
        boardId: 'b',
        type: 'view',
        title: 'Calendar view',
        fields: { viewType: 'calendar', dateDisplayPropertyId: 'due' },
        createAt: 1,
        updateAt: 1,
      },
    ]);
    expect(view.id).toBe('v');
    expect(view.fields.viewType).toBe('calendar');
    serve([], 201);
    const refused = await serverBackend('')
      .createView('b', { title: 'Calendar view', fields: { viewType: 'calendar' } as never })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(500);
  });

  it('sends two quick changes of one view one after the other', async () => {
    const order: string[] = [];
    vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
      order.push(`${init.method ?? 'GET'} ${typeof init.body === 'string' ? init.body : url}`);
      // The first change's request is the slower one: it would arrive last.
      await new Promise((resolve) => setTimeout(resolve, order.length === 2 ? 20 : 1));
      if (init.method === 'PATCH') return new Response(null, { status: 204 });
      return new Response(JSON.stringify([VIEW]), { status: 200 });
    });
    const backend = serverBackend('');
    await Promise.all([
      backend.patchView('b', 'v', { updatedFields: { cardOrder: ['a'] } }),
      backend.patchView('b', 'v', { updatedFields: { cardOrder: ['b'] } }),
    ]);
    expect(order.filter((line) => line.startsWith('PATCH'))).toEqual([
      'PATCH {"updatedFields":{"cardOrder":["a"]}}',
      'PATCH {"updatedFields":{"cardOrder":["b"]}}',
    ]);
    expect(order).toHaveLength(6);
    expect(order[3]).toMatch(/^GET/);
  });
});

describe('the trash on the server', () => {
  it('deletes a block and takes it back out by its own routes', async () => {
    const sent = serve(undefined, 204);
    const backend = serverBackend('');
    await backend.deleteBlock('b', 'c1');
    const restoring = serve({ ...BLOCK, id: 'c1', type: 'card', title: 'Back' });
    await backend.restoreBlock('b', 'c1');
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'DELETE /api/v1/w/w1/boards/b/blocks/c1',
    ]);
    expect(restoring.map((one) => `${one.method} ${one.url}`)).toEqual([
      'POST /api/v1/w/w1/boards/b/blocks/c1/undelete',
    ]);
    expect(sent[0]?.headers['X-Requested-With']).toBe('XMLHttpRequest');
  });
});

describe('the settings on the server', () => {
  it('are read and written under the contract’s names', async () => {
    const stored = {
      theme: 'dark',
      cardOpen: 'center',
      density: 'compact',
      haptics: false,
      timeZone: 'Asia/Seoul',
      weekStart: 'monday',
      hourCycle: 'h23',
    };
    serve(stored);
    const backend = serverBackend('');
    expect(await backend.settings()).toMatchObject({ theme: 'dark', haptics: false });
    const sent = serve({ ...stored, theme: 'light' });
    await backend.patchSettings({ theme: 'light', haptics: true });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'PATCH /api/v1/users/me/settings',
    ]);
    expect(sent[0]?.body).toEqual({ theme: 'light', haptics: true });
  });
});

/** Answers each request by the first route whose `METHOD path` starts it, and keeps what was sent. */
function serveRoutes(routes: [string, unknown, number?][]): Sent[] {
  const sent: Sent[] = [];
  vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
    const method = init.method ?? 'GET';
    sent.push({
      url,
      method,
      headers: init.headers as Record<string, string>,
      body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
    });
    const route = routes.find(([start]) => `${method} ${url}`.startsWith(start));
    if (!route) throw new Error(`nothing serves ${method} ${url}`);
    const [, answer, status = 200] = route;
    return Promise.resolve(
      new Response(status === 204 ? null : JSON.stringify(answer), {
        status,
        headers: {
          'Content-Type': status >= 400 ? 'application/problem+json' : 'application/json',
        },
      }),
    );
  });
  return sent;
}

const sentLines = (sent: Sent[]) => sent.map((one) => `${one.method} ${one.url}`);

describe('the sidebar on the server', () => {
  const CATEGORY = {
    id: 'k1',
    name: 'Work',
    userId: 'me',
    workspaceId: 't',
    type: 'custom',
    collapsed: true,
    sortOrder: 10,
    createAt: 1,
    updateAt: 2,
    deleteAt: 0,
  };
  const FILED = [
    { boardId: 'b1', hidden: false },
    { boardId: 'b2', hidden: true },
    { boardId: 'b3', hidden: false },
  ];
  const CATEGORIES = [{ ...CATEGORY, boardMetadata: FILED }];

  it('makes a category of the reader, under their identifier', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/users/me', { id: 'me', username: 'me', createAt: 1, updateAt: 1, deleteAt: 0 }],
      ['POST /api/v1/w/t/categories', CATEGORY, 201],
    ]);
    const made = await serverBackend('').createCategory('t', { name: 'Work' });
    expect(sentLines(sent)).toEqual(['GET /api/v1/users/me', 'POST /api/v1/w/t/categories']);
    expect(sent[1]?.body).toEqual({ name: 'Work', userId: 'me', workspaceId: 't' });
    expect(made).toMatchObject({ id: 'k1', boardIds: [], hiddenBoardIds: [] });
  });

  it('renames a category with its folding as it is, and keeps its boards', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/w/t/categories', CATEGORIES],
      ['PUT /api/v1/w/t/categories/k1', { ...CATEGORY, name: 'Home' }],
    ]);
    const renamed = await serverBackend('').patchCategory('t', 'k1', { name: 'Home' });
    expect(sentLines(sent)).toEqual([
      'GET /api/v1/w/t/categories',
      'PUT /api/v1/w/t/categories/k1',
    ]);
    expect(sent[1]?.body).toEqual({
      id: 'k1',
      userId: 'me',
      workspaceId: 't',
      name: 'Home',
      collapsed: true,
    });
    expect(renamed).toMatchObject({ name: 'Home', boardIds: ['b1', 'b2', 'b3'] });
    expect(renamed.hiddenBoardIds).toEqual(['b2']);
  });

  it('folds a category with its name as it is', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/w/t/categories', CATEGORIES],
      ['PUT /api/v1/w/t/categories/k1', { ...CATEGORY, collapsed: false }],
    ]);
    await serverBackend('').patchCategory('t', 'k1', { collapsed: false });
    expect(sent[1]?.body).toMatchObject({ name: 'Work', collapsed: false });
  });

  it('says 404 for a category the reader does not have, and sends no change', async () => {
    const sent = serveRoutes([['GET /api/v1/w/t/categories', CATEGORIES]]);
    const refused = await serverBackend('')
      .patchCategory('t', 'k9', { name: 'X' })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(404);
    expect(sentLines(sent)).toEqual(['GET /api/v1/w/t/categories']);
  });

  it('deletes a category and orders the categories by their routes', async () => {
    const sent = serveRoutes([
      ['DELETE /api/v1/w/t/categories/k1', undefined, 204],
      ['PUT /api/v1/w/t/category-order', ['k2', 'k1']],
    ]);
    const backend = serverBackend('');
    await backend.deleteCategory('t', 'k1');
    await backend.reorderCategories('t', ['k2', 'k1']);
    expect(sentLines(sent)).toEqual([
      'DELETE /api/v1/w/t/categories/k1',
      'PUT /api/v1/w/t/category-order',
    ]);
    expect(sent[1]?.body).toEqual(['k2', 'k1']);
  });

  it('files a board under a category, then puts it at its place among the category’s boards', async () => {
    const sent = serveRoutes([
      ['POST /api/v1/w/t/categories/k1/boards/b9', undefined, 204],
      [
        'GET /api/v1/w/t/categories',
        [{ ...CATEGORY, boardMetadata: [...FILED, { boardId: 'b9', hidden: false }] }],
      ],
      ['PUT /api/v1/w/t/categories/k1/board-order', ['b1', 'b9', 'b2', 'b3']],
    ]);
    await serverBackend('').moveBoardToCategory('t', 'b9', 'k1', 1);
    expect(sentLines(sent)).toEqual([
      'POST /api/v1/w/t/categories/k1/boards/b9',
      'GET /api/v1/w/t/categories',
      'PUT /api/v1/w/t/categories/k1/board-order',
    ]);
    expect(sent[2]?.body).toEqual(['b1', 'b9', 'b2', 'b3']);
  });

  it('sends the sidebar’s writes of a workspace one after another, each reading after the last', async () => {
    const sent = serveRoutes([
      ['POST /api/v1/w/t/categories/k1/boards/', undefined, 204],
      ['GET /api/v1/w/t/categories', CATEGORIES],
      ['PUT /api/v1/w/t/categories/k1', CATEGORY],
    ]);
    const backend = serverBackend('');
    await Promise.all([
      backend.moveBoardToCategory('t', 'b3', 'k1', 0),
      backend.patchCategory('t', 'k1', { collapsed: false }),
      backend.moveBoardToCategory('t', 'b2', 'k1', 0),
    ]);
    expect(sentLines(sent)).toEqual([
      'POST /api/v1/w/t/categories/k1/boards/b3',
      'GET /api/v1/w/t/categories',
      'PUT /api/v1/w/t/categories/k1/board-order',
      'GET /api/v1/w/t/categories',
      'PUT /api/v1/w/t/categories/k1',
      'POST /api/v1/w/t/categories/k1/boards/b2',
      'GET /api/v1/w/t/categories',
      'PUT /api/v1/w/t/categories/k1/board-order',
    ]);
  });

  it('hides a board and shows it again by its routes', async () => {
    const sent = serveRoutes([['PUT /api/v1/w/t/categories/k1/boards/b1/', undefined, 204]]);
    const backend = serverBackend('');
    await backend.setBoardHidden('t', 'k1', 'b1', true);
    await backend.setBoardHidden('t', 'k1', 'b1', false);
    expect(sentLines(sent)).toEqual([
      'PUT /api/v1/w/t/categories/k1/boards/b1/hide',
      'PUT /api/v1/w/t/categories/k1/boards/b1/unhide',
    ]);
  });
});

describe('boards on the server', () => {
  const BOARD = {
    id: 'b1',
    workspaceId: 't',
    createdBy: 'me',
    modifiedBy: 'me',
    type: 'P',
    minimumRole: 'editor',
    title: 'Plan',
    description: '',
    icon: '',
    showDescription: false,
    isTemplate: false,
    templateVersion: 0,
    properties: {},
    cardProperties: [],
    createAt: 1,
    updateAt: 2,
    deleteAt: 0,
  };
  const VIEW = {
    ...BLOCK,
    id: 'v1',
    boardId: 'b1',
    parentId: '',
    type: 'view',
    title: 'Board view',
  };

  it('makes a board and its first view in one request, the view tied to the board', async () => {
    const sent = serveRoutes([
      ['POST /api/v1/w/t/boards-and-blocks', { boards: [BOARD], blocks: [VIEW] }, 201],
    ]);
    const made = await serverBackend('').createBoard(
      't',
      { title: 'Plan', cardProperties: [{ id: 's', name: 'Status', type: 'select', options: [] }] },
      { title: 'Board view', fields: { viewType: 'board' } as never },
    );
    expect(sentLines(sent)).toEqual(['POST /api/v1/w/t/boards-and-blocks']);
    const body = sent[0]?.body as { boards: { id: string }[]; blocks: { boardId: string }[] };
    expect(body.boards).toEqual([
      {
        id: body.blocks[0]?.boardId,
        type: 'P',
        title: 'Plan',
        description: '',
        icon: '',
        cardProperties: [{ id: 's', name: 'Status', type: 'select', options: [] }],
      },
    ]);
    expect(body.blocks[0]).toMatchObject({ type: 'view', title: 'Board view' });
    expect(body.blocks[0]).not.toHaveProperty('parentId');
    expect(made.board.id).toBe('b1');
    expect(made.view).toMatchObject({ id: 'v1', title: 'Board view' });
  });

  it('changes, deletes and brings back a board by its routes', async () => {
    const sent = serveRoutes([
      ['PATCH /api/v1/w/w1/boards/b1', { ...BOARD, title: 'New' }],
      ['DELETE /api/v1/w/w1/boards/b1', undefined, 204],
      ['POST /api/v1/w/w1/boards/b1/undelete', BOARD],
    ]);
    const backend = serverBackend('');
    expect((await backend.patchBoard('b1', { title: 'New', minimumRole: 'viewer' })).title).toBe(
      'New',
    );
    await backend.deleteBoard('b1');
    expect((await backend.restoreBoard('b1')).id).toBe('b1');
    expect(sentLines(sent)).toEqual([
      'PATCH /api/v1/w/w1/boards/b1',
      'DELETE /api/v1/w/w1/boards/b1',
      'POST /api/v1/w/w1/boards/b1/undelete',
    ]);
    expect(sent[0]?.body).toEqual({ title: 'New', minimumRole: 'viewer' });
  });

  it('refuses a minimum role the contract does not have, sending nothing', async () => {
    const sent = serveRoutes([]);
    const refused = await serverBackend('')
      .patchBoard('b1', { minimumRole: 'admin' })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(400);
    expect(sent).toEqual([]);
  });

  it('makes a view as a block of the board', async () => {
    const sent = serveRoutes([['POST /api/v1/w/w1/boards/b1/blocks', [VIEW], 201]]);
    const view = await serverBackend('').createView('b1', {
      title: 'Table view',
      fields: { viewType: 'table' } as never,
    });
    expect(sent[0]?.body).toEqual([
      {
        boardId: 'b1',
        type: 'view',
        title: 'Table view',
        fields: { viewType: 'table' },
        createAt: 1,
        updateAt: 1,
      },
    ]);
    expect(view.id).toBe('v1');
  });
});

describe('the password on the server', () => {
  it('is changed with the old one, and a wrong old one is let through with its reason', async () => {
    const sent = serveRoutes([['POST /api/v1/users/me/password', undefined, 204]]);
    const backend = serverBackend('');
    await backend.changePassword('old secret', 'new secret');
    expect(sentLines(sent)).toEqual(['POST /api/v1/users/me/password']);
    expect(sent[0]?.body).toEqual({ oldPassword: 'old secret', newPassword: 'new secret' });
    serveRoutes([
      [
        'POST /api/v1/users/me/password',
        {
          type: '/problems/invalid-credentials',
          title: 'Invalid credentials',
          status: 400,
          detail: 'wrong',
          reason: 'invalid_credentials',
          errors: [{ pointer: '/oldPassword', detail: 'wrong', reason: 'invalid_credentials' }],
        },
        400,
      ],
    ]);
    const refused = await backend
      .changePassword('x', 'new secret')
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.reason).toBe('invalid_credentials');
    expect(isBackendError(refused) && refused.refusalAt('/oldPassword')?.reason).toBe(
      'invalid_credentials',
    );
  });
});

describe('sharing a board on the server', () => {
  const MEMBER = {
    boardId: 'b1',
    userId: 'u2',
    schemeAdmin: false,
    schemeEditor: false,
    schemeCommenter: true,
    schemeViewer: false,
  };

  it('adds, changes and removes a member with exactly the one role', async () => {
    const sent = serveRoutes([
      ['POST /api/v1/w/w1/boards/b1/members', MEMBER],
      [
        'PUT /api/v1/w/w1/boards/b1/members/u2',
        { ...MEMBER, schemeCommenter: false, schemeViewer: true },
      ],
      ['DELETE /api/v1/w/w1/boards/b1/members/u2', undefined, 204],
    ]);
    const backend = serverBackend('');
    expect((await backend.addMember('b1', 'u2', 'commenter')).schemeCommenter).toBe(true);
    expect((await backend.setMemberRole('b1', 'u2', 'viewer')).schemeViewer).toBe(true);
    await backend.removeMember('b1', 'u2');
    expect(sentLines(sent)).toEqual([
      'POST /api/v1/w/w1/boards/b1/members',
      'PUT /api/v1/w/w1/boards/b1/members/u2',
      'DELETE /api/v1/w/w1/boards/b1/members/u2',
    ]);
    expect(sent[0]?.body).toEqual({
      userId: 'u2',
      schemeAdmin: false,
      schemeEditor: false,
      schemeCommenter: true,
      schemeViewer: false,
    });
    expect(sent[1]?.body).toEqual({
      schemeAdmin: false,
      schemeEditor: false,
      schemeCommenter: false,
      schemeViewer: true,
    });
  });

  it('reads and sets the public link, asking for a new token only when told', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/w/w1/boards/b1/sharing', { enabled: false }],
      ['POST /api/v1/w/w1/boards/b1/sharing', { enabled: true, token: 'tok', updateAt: 3 }],
    ]);
    const backend = serverBackend('');
    expect(await backend.sharing('b1')).toEqual({ enabled: false, token: '' });
    expect(await backend.setSharing('b1', { enabled: true })).toEqual({
      enabled: true,
      token: 'tok',
    });
    await backend.setSharing('b1', { enabled: true, regenerate: true });
    expect(sent.slice(1).map((one) => one.body)).toEqual([
      { enabled: true },
      { enabled: true, regenerate: true },
    ]);
  });

  it('finds people of the workspace by the search of their list', async () => {
    const sent = serveRoutes([['GET /api/v1/w/t/users', []]]);
    await serverBackend('').searchUsers('t', 'ann lee');
    expect(sentLines(sent)).toEqual(['GET /api/v1/w/t/users?search=ann+lee']);
  });

  it("takes the reader's workspaces from their workspaces, those they are a guest of too, with what they are in each", async () => {
    const workspace = (id: string, role: string) => ({ id, orgId: 'o', name: `n-${id}`, role });
    const sent = serveRoutes([
      [
        'GET /api/v1/users/me/workspaces',
        [workspace('w1', 'guest'), workspace('w2', 'member'), workspace('w3', 'admin')],
      ],
    ]);
    expect(await serverBackend('').workspaces()).toEqual([
      { id: 'w1', title: 'n-w1', role: 'guest' },
      { id: 'w2', title: 'n-w2', role: 'member' },
      { id: 'w3', title: 'n-w3', role: 'admin' },
    ]);
    expect(sentLines(sent)).toEqual(['GET /api/v1/users/me/workspaces']);
  });
});

describe('templates on the server', () => {
  const BOARD = {
    id: 'b2',
    workspaceId: 't',
    createdBy: 'me',
    modifiedBy: 'me',
    type: 'P',
    minimumRole: 'editor',
    title: 'Project Tasks',
    description: '',
    icon: '',
    showDescription: false,
    isTemplate: false,
    isBuiltin: false,
    templateVersion: 0,
    properties: {},
    cardProperties: [],
    createAt: 1,
    updateAt: 2,
    deleteAt: 0,
  };

  it('reads the templates, copies a board either way, and starts the welcome board', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/w/t/templates', [{ ...BOARD, id: 'tp', isTemplate: true, isBuiltin: true }]],
      ['POST /api/v1/w/w1/boards/', { boards: [BOARD], blocks: [] }, 201],
      ['POST /api/v1/w/t/onboard', { workspaceId: 't', boardId: 'w1' }],
    ]);
    const backend = serverBackend('');
    const [template] = await backend.templates('t');
    expect(template).toMatchObject({ id: 'tp', isTemplate: true, isBuiltin: true });
    expect((await backend.duplicateBoard('tp', false, 't')).id).toBe('b2');
    await backend.duplicateBoard('b2', true, 't');
    expect(await backend.onboard('t')).toBe('w1');
    expect(sentLines(sent)).toEqual([
      'GET /api/v1/w/t/templates',
      'POST /api/v1/w/w1/boards/tp/duplicate?asTemplate=false&toWorkspace=t',
      'POST /api/v1/w/w1/boards/b2/duplicate?asTemplate=true&toWorkspace=t',
      'POST /api/v1/w/t/onboard',
    ]);
  });
});

describe('searching boards on the server', () => {
  it('asks the workspace’s search by title', async () => {
    const sent = serveRoutes([['GET /api/v1/w/t/boards/search', []]]);
    expect(await serverBackend('').searchBoards('t', 'garden plan')).toEqual([]);
    expect(sentLines(sent)).toEqual(['GET /api/v1/w/t/boards/search?q=garden+plan']);
  });
});
