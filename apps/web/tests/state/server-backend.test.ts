/**
 * The backend that calls the server, against a stand-in for `fetch`: what
 * it sends, and how the server's shapes become the screens'. What the real
 * server does with it is the work of `e2e-live/`.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
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

  it('makes a card from a draft, and refuses one with a body before sending it', async () => {
    const sent = serve({
      ...BLOCK,
      id: 'c1',
      title: 'New',
      icon: '',
      isTemplate: false,
      properties: { status: 'todo' },
      contentOrder: [],
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

    const refused = await backend
      .createCard('b', { body: { type: 'doc', content: [{ type: 'paragraph' }] } })
      .catch((error: unknown) => error);
    expect(isBackendError(refused) && refused.reason).toBe('not_wired');
    expect(sent).toHaveLength(1);
  });

  it("reads a card's comments and attachments as blocks under it", async () => {
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
    contentOrder: [],
  };

  /** A server of one card whose values are `values`; it keeps what it is sent. */
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
        const patch = body as { title?: string; updatedProperties?: Record<string, unknown> };
        const replaced = patch.updatedProperties && Object.keys(patch.updatedProperties).length > 0;
        card = {
          ...card,
          title: patch.title ?? card.title,
          properties: replaced ? (patch.updatedProperties ?? {}) : card.properties,
        };
      }
      if (init.method === 'PATCH' && url.endsWith('/blocks/c1')) {
        const fields = (body as { updatedFields: { properties?: Record<string, unknown> } })
          .updatedFields;
        card = { ...card, properties: fields.properties ?? card.properties };
        return new Response(null, { status: 204 });
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

  it('sends every value of the card, the ones it keeps too', async () => {
    const { sent, card } = oneCard({ a: '1', b: '2' });
    await new ServerBackend('').patchCard('b', 'c1', {
      updatedProperties: { b: '3' },
      deletedProperties: ['a'],
    });
    expect(sent.at(-1)?.body).toEqual({ updatedProperties: { b: '3' } });
    expect(card().properties).toEqual({ b: '3' });
  });

  it('takes away the last value through the block', async () => {
    const { sent, card } = oneCard({ a: '1' });
    const answer = await new ServerBackend('').patchCard('b', 'c1', { deletedProperties: ['a'] });
    expect(sent.map((one) => `${one.method} ${one.url}`)).toEqual([
      'GET /api/v1/cards/c1',
      'PATCH /api/v1/boards/b/blocks/c1',
      'GET /api/v1/cards/c1',
    ]);
    expect(card().properties).toEqual({});
    expect(answer.fields.properties).toEqual({});
  });

  it('sends two quick changes of one card one after the other, so neither undoes the other', async () => {
    const { card } = oneCard({});
    const backend = new ServerBackend('');
    await Promise.all([
      backend.patchCard('b', 'c1', { updatedProperties: { a: '1' } }),
      backend.patchCard('b', 'c1', { updatedProperties: { b: '2' } }),
    ]);
    expect(card().properties).toEqual({ a: '1', b: '2' });
  });

  it('clears the last value with a new title: the title by the card patch, the values by the block', async () => {
    const { sent, card } = oneCard({ a: '1' });
    await new ServerBackend('').patchCard('b', 'c1', { title: 'T', deletedProperties: ['a'] });
    expect(sent.filter((one) => one.method === 'PATCH').map((one) => [one.url, one.body])).toEqual([
      ['/api/v1/cards/c1', { title: 'T' }],
      ['/api/v1/boards/b/blocks/c1', { updatedFields: { properties: {} } }],
    ]);
    expect(card()).toMatchObject({ title: 'T', properties: {} });
  });

  it('sends nothing when the values come out as they were', async () => {
    const { sent } = oneCard({});
    const answer = await new ServerBackend('').patchCard('b', 'c1', { deletedProperties: ['x'] });
    expect(sent.map((one) => one.method)).toEqual(['GET']);
    expect(answer.id).toBe('c1');
  });

  it('keeps a clearing that was saved when only reading the card back fails', async () => {
    let calls = 0;
    vi.stubGlobal('fetch', (_url: string, init: RequestInit) => {
      calls += 1;
      if (calls === 3) return Promise.reject(new TypeError('offline'));
      if (init.method === 'PATCH') return Promise.resolve(new Response(null, { status: 204 }));
      return Promise.resolve(
        new Response(JSON.stringify({ ...CARD, properties: { a: '1' } }), { status: 200 }),
      );
    });
    const answer = await new ServerBackend('').patchCard('b', 'c1', { deletedProperties: ['a'] });
    expect(calls).toBe(3);
    expect(answer.fields.properties).toEqual({});
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
