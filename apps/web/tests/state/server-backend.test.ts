/**
 * The backend that calls the server, against a stand-in for `fetch`: what
 * it sends, and how the server's shapes become the screens'. What the real
 * server does with it is the work of `e2e-live/`.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
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
        headers: { 'Content-Type': 'application/json' },
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
    await new ServerBackend('/base').signIn('ann@home', 'secret pass');
    expect(sent).toHaveLength(1);
    expect(sent[0]?.url).toBe('/base/api/v1/login');
    expect(sent[0]?.body).toEqual({
      username: 'ann@home',
      email: 'ann@home',
      password: 'secret pass',
    });
    expect(sent[0]?.headers['X-Requested-With']).toBe('XMLHttpRequest');
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
    const backend = new ServerBackend('');
    const card = await backend.createCard('b', { properties: { status: 'todo' } });
    expect(sent[0]).toMatchObject({
      url: '/api/v1/boards/b/cards',
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
    const comment = await new ServerBackend('').createComment('b', 'c', body);
    expect(sent[0]).toMatchObject({
      url: '/api/v1/boards/b/blocks',
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
    const changed = await new ServerBackend('').patchComment('b', 'm', { body, readAt: 7 });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'GET /api/v1/boards/b/blocks?blockId=m',
      'PATCH /api/v1/boards/b/blocks/m',
      'GET /api/v1/boards/b/blocks?blockId=m',
    ]);
    expect(sent[1]?.body).toEqual({ body, readAt: 7 });
    expect(changed).toMatchObject({ id: 'm', body, updateAt: 9 });

    // No text is no comment: refused before anything is sent.
    const none = serve([]);
    const empty = await new ServerBackend('')
      .patchComment('b', 'm', { body: null, readAt: 1 })
      .catch((error: unknown) => error);
    expect(isBackendError(empty) && empty.status).toBe(400);
    expect(none).toHaveLength(0);

    // A block that is not a comment is not changed.
    const other = serve([{ ...BLOCK, id: 'c', type: 'card', title: 'x' }]);
    const refused = await new ServerBackend('')
      .patchComment('b', 'c', { body, readAt: 1 })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(404);
    expect(other.map((one) => one.method)).toEqual(['GET']);
  });

  it("reads a card's comments and attachments as blocks under it", async () => {
    const body = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Body' }] }],
    };
    serve([{ ...BLOCK, id: 'n', type: 'comment', title: '', body }]);
    expect((await new ServerBackend('').comments('b', 'c'))[0]?.body).toEqual(body);
    // One stored before comments had bodies: its title, a paragraph a line.
    const sent = serve([{ ...BLOCK, id: 'm', type: 'comment', title: 'Looks good\nShip it' }]);
    const backend = new ServerBackend('');
    const [comment] = await backend.comments('b', 'c');
    expect(sent[0]?.url).toBe('/api/v1/boards/b/blocks?parentId=c&type=comment');
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
    await new ServerBackend('').patchCard('b', 'c1', { title: 'New' });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual(['PATCH /api/v1/cards/c1']);
    expect(sent[0]?.body).toEqual({ title: 'New' });
  });

  it('sends the values it sets and the ones it clears, and nothing else, without reading the card', async () => {
    const { sent, card } = oneCard({ a: '1', b: '2', c: '3' });
    const answer = await new ServerBackend('').patchCard('b', 'c1', {
      updatedProperties: { b: '4' },
      deletedProperties: ['a'],
    });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual(['PATCH /api/v1/cards/c1']);
    expect(sent[0]?.body).toEqual({ updatedProperties: { b: '4' }, deletedProperties: ['a'] });
    expect(card().properties).toEqual({ b: '4', c: '3' });
    expect(answer.fields.properties).toEqual({ b: '4', c: '3' });
  });

  it('takes away the last value by the card patch, with a new title', async () => {
    const { sent, card } = oneCard({ a: '1' });
    await new ServerBackend('').patchCard('b', 'c1', { title: 'T', deletedProperties: ['a'] });
    expect(sent.map((one) => [one.method, one.url, one.body])).toEqual([
      ['PATCH', '/api/v1/cards/c1', { title: 'T', deletedProperties: ['a'] }],
    ]);
    expect(card()).toMatchObject({ title: 'T', properties: {} });
  });

  it('sends two quick changes of one card one after the other, in the order they were made', async () => {
    const { sent, card } = oneCard({});
    const backend = new ServerBackend('');
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
    serve({ ...CARD, properties: {}, hasBody: true, tasksTotal: 3, tasksDone: 1 });
    const answer = await new ServerBackend('').patchCard('b', 'c1', { title: 'x' });
    expect(answer).toMatchObject({ hasBody: true, tasksTotal: 3, tasksDone: 1 });
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
    const member = await new ServerBackend('').joinBoard('b');
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual(['POST /api/v1/boards/b/join']);
    expect(member).toMatchObject({ boardId: 'b', userId: 'u', schemeEditor: true });
  });
});

describe('writing a card’s body on the server', () => {
  const body = {
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hello' }] }],
  };
  const CARD = {
    ...BLOCK,
    id: 'c1',
    title: 'Card',
    icon: '',
    isTemplate: false,
    properties: {},
    hasBody: true,
    tasksTotal: 0,
    tasksDone: 0,
  };

  it('sends the body with the version it was read at, and gives the stored card', async () => {
    const sent = serve({ ...CARD, body, updateAt: 8 });
    const stored = await new ServerBackend('').writeCardBody('b', 'c1', { body, readAt: 5 });
    expect(sent.map((one) => [one.method, one.url, one.body])).toEqual([
      ['PATCH', '/api/v1/cards/c1', { body, readAt: 5 }],
    ]);
    expect(stored).toMatchObject({ id: 'c1', body, updateAt: 8, hasBody: true });
  });

  it('sends no body as null, and lets a refusal for a stale version through as 409', async () => {
    const sent = serve({ error: 'conflict', message: 'stale' }, 409);
    const refused = await new ServerBackend('')
      .writeCardBody('b', 'c1', { body: null, readAt: 5 })
      .catch((error: unknown) => error);
    expect(sent[0]?.body).toEqual({ body: null, readAt: 5 });
    expect(isBackendError(refused) && refused.status).toBe(409);
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
    const view = await new ServerBackend('').patchView('b', 'v', {
      updatedFields: { cardOrder: ['c2', 'c1'] },
      deleteAt: 0,
    });
    expect(sent.map((one) => [one.method, one.url, one.body])).toEqual([
      ['GET', '/api/v1/boards/b/blocks?blockId=v', undefined],
      ['PATCH', '/api/v1/boards/b/blocks/v', { updatedFields: { cardOrder: ['c2', 'c1'] } }],
      ['GET', '/api/v1/boards/b/blocks?blockId=v', undefined],
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
    const refused = await new ServerBackend('')
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
    const view = await new ServerBackend('').patchView('b', 'v', {
      updatedFields: { cardOrder: ['c2', 'c1'] },
    });
    expect(view.fields.cardOrder).toEqual(['c2', 'c1']);
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
    const backend = new ServerBackend('');
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
    const backend = new ServerBackend('');
    await backend.deleteBlock('b', 'c1');
    const restoring = serve({ ...BLOCK, id: 'c1', type: 'card', title: 'Back' });
    await backend.restoreBlock('b', 'c1');
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'DELETE /api/v1/boards/b/blocks/c1',
    ]);
    expect(restoring.map((one) => `${one.method} ${one.url}`)).toEqual([
      'POST /api/v1/boards/b/blocks/c1/undelete',
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
    const backend = new ServerBackend('');
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
        headers: { 'Content-Type': 'application/json' },
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
    teamId: 't',
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
      ['POST /api/v1/teams/t/categories', CATEGORY, 201],
    ]);
    const made = await new ServerBackend('').createCategory('t', { name: 'Work' });
    expect(sentLines(sent)).toEqual(['GET /api/v1/users/me', 'POST /api/v1/teams/t/categories']);
    expect(sent[1]?.body).toEqual({ name: 'Work', userId: 'me', teamId: 't' });
    expect(made).toMatchObject({ id: 'k1', boardIds: [], hiddenBoardIds: [] });
  });

  it('renames a category with its folding as it is, and keeps its boards', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/teams/t/categories', CATEGORIES],
      ['PUT /api/v1/teams/t/categories/k1', { ...CATEGORY, name: 'Home' }],
    ]);
    const renamed = await new ServerBackend('').patchCategory('t', 'k1', { name: 'Home' });
    expect(sentLines(sent)).toEqual([
      'GET /api/v1/teams/t/categories',
      'PUT /api/v1/teams/t/categories/k1',
    ]);
    expect(sent[1]?.body).toEqual({
      id: 'k1',
      userId: 'me',
      teamId: 't',
      name: 'Home',
      collapsed: true,
    });
    expect(renamed).toMatchObject({ name: 'Home', boardIds: ['b1', 'b2', 'b3'] });
    expect(renamed.hiddenBoardIds).toEqual(['b2']);
  });

  it('folds a category with its name as it is', async () => {
    const sent = serveRoutes([
      ['GET /api/v1/teams/t/categories', CATEGORIES],
      ['PUT /api/v1/teams/t/categories/k1', { ...CATEGORY, collapsed: false }],
    ]);
    await new ServerBackend('').patchCategory('t', 'k1', { collapsed: false });
    expect(sent[1]?.body).toMatchObject({ name: 'Work', collapsed: false });
  });

  it('says 404 for a category the reader does not have, and sends no change', async () => {
    const sent = serveRoutes([['GET /api/v1/teams/t/categories', CATEGORIES]]);
    const refused = await new ServerBackend('')
      .patchCategory('t', 'k9', { name: 'X' })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(404);
    expect(sentLines(sent)).toEqual(['GET /api/v1/teams/t/categories']);
  });

  it('deletes a category and orders the categories by their routes', async () => {
    const sent = serveRoutes([
      ['DELETE /api/v1/teams/t/categories/k1', undefined, 204],
      ['PUT /api/v1/teams/t/category-order', ['k2', 'k1']],
    ]);
    const backend = new ServerBackend('');
    await backend.deleteCategory('t', 'k1');
    await backend.reorderCategories('t', ['k2', 'k1']);
    expect(sentLines(sent)).toEqual([
      'DELETE /api/v1/teams/t/categories/k1',
      'PUT /api/v1/teams/t/category-order',
    ]);
    expect(sent[1]?.body).toEqual(['k2', 'k1']);
  });

  it('files a board under a category, then puts it at its place among the category’s boards', async () => {
    const sent = serveRoutes([
      ['POST /api/v1/teams/t/categories/k1/boards/b9', undefined, 204],
      [
        'GET /api/v1/teams/t/categories',
        [{ ...CATEGORY, boardMetadata: [...FILED, { boardId: 'b9', hidden: false }] }],
      ],
      ['PUT /api/v1/teams/t/categories/k1/board-order', ['b1', 'b9', 'b2', 'b3']],
    ]);
    await new ServerBackend('').moveBoardToCategory('t', 'b9', 'k1', 1);
    expect(sentLines(sent)).toEqual([
      'POST /api/v1/teams/t/categories/k1/boards/b9',
      'GET /api/v1/teams/t/categories',
      'PUT /api/v1/teams/t/categories/k1/board-order',
    ]);
    expect(sent[2]?.body).toEqual(['b1', 'b9', 'b2', 'b3']);
  });

  it('sends the sidebar’s writes of a team one after another, each reading after the last', async () => {
    const sent = serveRoutes([
      ['POST /api/v1/teams/t/categories/k1/boards/', undefined, 204],
      ['GET /api/v1/teams/t/categories', CATEGORIES],
      ['PUT /api/v1/teams/t/categories/k1', CATEGORY],
    ]);
    const backend = new ServerBackend('');
    await Promise.all([
      backend.moveBoardToCategory('t', 'b3', 'k1', 0),
      backend.patchCategory('t', 'k1', { collapsed: false }),
      backend.moveBoardToCategory('t', 'b2', 'k1', 0),
    ]);
    expect(sentLines(sent)).toEqual([
      'POST /api/v1/teams/t/categories/k1/boards/b3',
      'GET /api/v1/teams/t/categories',
      'PUT /api/v1/teams/t/categories/k1/board-order',
      'GET /api/v1/teams/t/categories',
      'PUT /api/v1/teams/t/categories/k1',
      'POST /api/v1/teams/t/categories/k1/boards/b2',
      'GET /api/v1/teams/t/categories',
      'PUT /api/v1/teams/t/categories/k1/board-order',
    ]);
  });

  it('hides a board and shows it again by its routes', async () => {
    const sent = serveRoutes([['PUT /api/v1/teams/t/categories/k1/boards/b1/', undefined, 204]]);
    const backend = new ServerBackend('');
    await backend.setBoardHidden('t', 'k1', 'b1', true);
    await backend.setBoardHidden('t', 'k1', 'b1', false);
    expect(sentLines(sent)).toEqual([
      'PUT /api/v1/teams/t/categories/k1/boards/b1/hide',
      'PUT /api/v1/teams/t/categories/k1/boards/b1/unhide',
    ]);
  });
});

describe('boards on the server', () => {
  const BOARD = {
    id: 'b1',
    teamId: 't',
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
      ['POST /api/v1/boards-and-blocks', { boards: [BOARD], blocks: [VIEW] }, 201],
    ]);
    const made = await new ServerBackend('').createBoard(
      't',
      { title: 'Plan', cardProperties: [{ id: 's', name: 'Status', type: 'select', options: [] }] },
      { title: 'Board view', fields: { viewType: 'board' } as never },
    );
    expect(sentLines(sent)).toEqual(['POST /api/v1/boards-and-blocks']);
    const body = sent[0]?.body as { boards: { id: string }[]; blocks: { boardId: string }[] };
    expect(body.boards).toEqual([
      {
        id: body.blocks[0]?.boardId,
        teamId: 't',
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
      ['PATCH /api/v1/boards/b1', { ...BOARD, title: 'New' }],
      ['DELETE /api/v1/boards/b1', undefined, 204],
      ['POST /api/v1/boards/b1/undelete', BOARD],
    ]);
    const backend = new ServerBackend('');
    expect((await backend.patchBoard('b1', { title: 'New', minimumRole: 'viewer' })).title).toBe(
      'New',
    );
    await backend.deleteBoard('b1');
    expect((await backend.restoreBoard('b1')).id).toBe('b1');
    expect(sentLines(sent)).toEqual([
      'PATCH /api/v1/boards/b1',
      'DELETE /api/v1/boards/b1',
      'POST /api/v1/boards/b1/undelete',
    ]);
    expect(sent[0]?.body).toEqual({ title: 'New', minimumRole: 'viewer' });
  });

  it('refuses a minimum role the contract does not have, sending nothing', async () => {
    const sent = serveRoutes([]);
    const refused = await new ServerBackend('')
      .patchBoard('b1', { minimumRole: 'admin' })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.status).toBe(400);
    expect(sent).toEqual([]);
  });

  it('makes a view as a block of the board', async () => {
    const sent = serveRoutes([['POST /api/v1/boards/b1/blocks', [VIEW], 201]]);
    const view = await new ServerBackend('').createView('b1', {
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
    const backend = new ServerBackend('');
    await backend.changePassword('old secret', 'new secret');
    expect(sentLines(sent)).toEqual(['POST /api/v1/users/me/password']);
    expect(sent[0]?.body).toEqual({ oldPassword: 'old secret', newPassword: 'new secret' });
    serveRoutes([
      [
        'POST /api/v1/users/me/password',
        {
          error: 'bad_request',
          message: 'wrong',
          reason: 'invalid_credentials',
          field: 'oldPassword',
        },
        400,
      ],
    ]);
    const refused = await backend
      .changePassword('x', 'new secret')
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.reason).toBe('invalid_credentials');
  });
});
