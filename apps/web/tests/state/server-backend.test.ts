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
    const sent = serve([{ ...BLOCK, id: 'm', type: 'comment', title: 'Looks good' }]);
    const backend = new ServerBackend('');
    const [comment] = await backend.comments('b', 'c');
    expect(sent[0]?.url).toBe('/api/v1/boards/b/blocks?parentId=c&type=comment');
    expect(comment?.body).toEqual({
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Looks good' }] }],
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
  });
});
