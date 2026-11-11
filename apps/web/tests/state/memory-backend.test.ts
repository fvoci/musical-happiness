/**
 * The in-memory backend behaves the way the server design says the server
 * will, as far as a screen can tell.
 */
import { describe, expect, it } from 'vitest';

import { BackendError, isConflict, type Change } from '$lib/state/index.ts';
import { LIMITS } from '$lib/state/memory/backend.ts';

import { loadTemplates, settle, setup, STATUS, text } from './support.ts';

async function firstBoard(made = setup()) {
  const [team] = await made.backend.teams();
  const [board] = await made.backend.boards(team?.id ?? '');
  if (!board) throw new Error('no board');
  const cards = await made.backend.cards(board.id);
  const byTitle = (title: string) => {
    const card = cards.find((each) => each.title === title);
    if (!card) throw new Error(`no card ${title}`);
    return card;
  };
  return { ...made, boardId: board.id, cards, byTitle };
}

const rejection = async (promise: Promise<unknown>): Promise<BackendError> => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof BackendError) return error;
    throw error;
  }
  throw new Error('the call did not reject');
};

describe('reading', () => {
  it('makes identifiers of its own, shaped like a UUID of version 7', async () => {
    const { cards, boardId } = await firstBoard();
    const shape = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
    expect(boardId).toMatch(shape);
    for (const card of cards) expect(card.id).toMatch(shape);
  });

  it('lists cards without their bodies, with what a list shows of them', async () => {
    const { byTitle, backend, boardId } = await firstBoard();
    const alpha = byTitle('Alpha');
    expect(alpha).not.toHaveProperty('body');
    expect(alpha.hasBody).toBe(true);
    expect(byTitle('Beta').hasBody).toBe(false);
    expect((await backend.card(boardId, alpha.id)).body).toEqual(text('first body'));
  });

  it('answers 404 for what is not there', async () => {
    const { backend, boardId } = await firstBoard();
    expect((await rejection(backend.board('nope'))).status).toBe(404);
    expect((await rejection(backend.card(boardId, 'nope'))).status).toBe(404);
  });

  it('hands out copies: changing an answer changes nothing stored', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Alpha').id);
    card.title = 'scribbled';
    card.fields.properties[STATUS] = 'done';
    const again = await backend.card(boardId, card.id);
    expect(again.title).toBe('Alpha');
    expect(again.fields.properties[STATUS]).toBe('todo');
  });
});

describe('writing a body', () => {
  it('stores it when the card is as the writer read it', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Beta').id);
    const stored = await backend.writeCardBody(boardId, card.id, {
      body: text('hello'),
      readAt: card.updateAt,
    });
    expect(stored.body).toEqual(text('hello'));
    expect(stored.updateAt).toBeGreaterThan(card.updateAt);
    expect(stored.hasBody).toBe(true);
  });

  it('refuses a write based on an older version with 409', async () => {
    const { backend, other, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Alpha').id);
    await other.writeCardBody(boardId, card.id, { body: text('theirs'), readAt: card.updateAt });
    const error = await rejection(
      backend.writeCardBody(boardId, card.id, { body: text('mine'), readAt: card.updateAt }),
    );
    expect(isConflict(error)).toBe(true);
    expect(error.status).toBe(409);
    expect((await backend.card(boardId, card.id)).body).toEqual(text('theirs'));
  });

  it('refuses a document that is not on the allow-list with 400 and the field', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Alpha').id);
    const error = await rejection(
      backend.writeCardBody(boardId, card.id, {
        body: { type: 'doc', content: [{ type: 'script' }] },
        readAt: card.updateAt,
      }),
    );
    expect(error.status).toBe(400);
    expect(error.errors).toEqual([{ pointer: '/body', detail: error.message, reason: 'invalid' }]);
    expect(error.refusalAt('/body')?.reason).toBe('invalid');
  });

  it('refuses an image that names a file of no board, or of another one', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Alpha').id);
    const image = { type: 'doc', content: [{ type: 'image', attrs: { fileId: 'nowhere' } }] };
    const error = await rejection(
      backend.writeCardBody(boardId, card.id, { body: image, readAt: card.updateAt }),
    );
    expect(error.status).toBe(400);
  });

  it('refuses a body over the limit with 400 naming the body, as the server does', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Alpha').id);
    const error = await rejection(
      backend.writeCardBody(boardId, card.id, {
        body: text('가'.repeat(400_000)),
        readAt: card.updateAt,
      }),
    );
    expect(error).toMatchObject({ status: 400, errors: [{ pointer: '/body' }] });
    expect(error.limit).toBe(LIMITS.bodyBytes);
  });

  it('stores an empty paragraph as no body', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const card = await backend.card(boardId, byTitle('Alpha').id);
    const stored = await backend.writeCardBody(boardId, card.id, {
      body: { type: 'doc', content: [{ type: 'paragraph' }] },
      readAt: card.updateAt,
    });
    expect(stored.body).toBeNull();
    expect(stored.hasBody).toBe(false);
  });
});

describe('deleting', () => {
  it('moves a card and what is under it to a trash they come back from', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const alpha = byTitle('Alpha');
    const before = await backend.comments(boardId, alpha.id);
    expect(before.length).toBeGreaterThan(0);
    await backend.deleteBlock(boardId, alpha.id);
    expect((await rejection(backend.card(boardId, alpha.id))).status).toBe(404);
    await backend.restoreBlock(boardId, alpha.id);
    expect((await backend.card(boardId, alpha.id)).title).toBe('Alpha');
    expect(await backend.comments(boardId, alpha.id)).toHaveLength(before.length);
  });

  it('answers 404 when asked to restore what is not in the trash', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    expect((await rejection(backend.restoreBlock(boardId, byTitle('Alpha').id))).status).toBe(404);
  });

  it('takes a board out of every sidebar and puts it back with its blocks', async () => {
    const { backend, boardId } = await firstBoard();
    const [team] = await backend.teams();
    await backend.deleteBoard(boardId);
    expect(await backend.boards(team?.id ?? '')).toHaveLength(0);
    await backend.restoreBoard(boardId);
    expect(await backend.cards(boardId)).toHaveLength(3);
  });
});

describe('settings', () => {
  it('always answers with every member, and resets one that is set to null', async () => {
    const { backend } = setup();
    const start = await backend.settings();
    expect(Object.keys(start).sort()).toEqual(
      ['theme', 'cardOpen', 'density', 'hourCycle', 'timeZone', 'haptics', 'weekStart'].sort(),
    );
    const changed = await backend.patchSettings({ theme: 'dark', timeZone: 'Asia/Seoul' });
    expect(changed).toMatchObject({ theme: 'dark', timeZone: 'Asia/Seoul', density: 'auto' });
    expect((await backend.patchSettings({ theme: null })).theme).toBe('auto');
  });

  it('refuses a time zone it does not know, and a value outside the set', async () => {
    const { backend } = setup();
    const zone = await rejection(backend.patchSettings({ timeZone: 'Mars/Olympus' }));
    expect(zone.status).toBe(400);
    expect(zone.errors.map((entry) => entry.pointer)).toEqual(['/timeZone']);
    const density = await rejection(
      backend.patchSettings({ density: 'huge' as unknown as 'auto' }),
    );
    expect(density.errors.map((entry) => entry.pointer)).toEqual(['/density']);
  });
});

describe('telling about changes', () => {
  it('tells everybody who watches the board, the writer included', async () => {
    const { backend, other, boardId, byTitle } = await firstBoard();
    const mine: Change[] = [];
    const theirs: Change[] = [];
    const stopMine = backend.watchBoard(boardId, (change) => mine.push(change));
    other.watchBoard(boardId, (change) => theirs.push(change));
    await other.patchCard(boardId, byTitle('Alpha').id, { title: 'Alpha!' });
    await settle();
    for (const seen of [mine, theirs]) {
      expect(seen).toHaveLength(1);
      expect(seen[0]).toMatchObject({ kind: 'blocks', blocks: [{ title: 'Alpha!' }] });
    }
    stopMine();
    await other.patchCard(boardId, byTitle('Alpha').id, { title: 'Alpha?' });
    await settle();
    expect(mine).toHaveLength(1);
    expect(theirs).toHaveLength(2);
  });
});

describe('files', () => {
  it('takes a file in, tells the progress, and gives an address for it', async () => {
    const { backend, boardId, byTitle } = await firstBoard();
    const seen: number[] = [];
    const file = new File(['hello'], 'hello.txt', { type: 'text/plain' });
    const uploaded = await backend.upload(boardId, file, {
      onProgress: ({ sentBytes }) => seen.push(sentBytes),
    });
    expect(uploaded).toMatchObject({ name: 'hello.txt', size: 5, contentType: 'text/plain' });
    expect(seen.at(-1)).toBe(5);
    expect(backend.fileUrl(boardId, uploaded.fileId)).toMatch(/^blob:/);
    const attachment = await backend.createAttachment(boardId, byTitle('Beta').id, uploaded);
    expect(attachment).toMatchObject({ type: 'attachment', name: 'hello.txt', size: 5 });
  });

  it('stops an upload that was given up', async () => {
    const { backend, boardId } = await firstBoard();
    const controller = new AbortController();
    controller.abort();
    const error = await rejection(
      backend.upload(boardId, new File(['x'], 'x.txt'), { signal: controller.signal }),
    );
    expect(error.status).toBe(0);
  });

  it('leaves the failure asked for the next upload to that one, not to one given up meanwhile', async () => {
    const { backend, boardId } = await firstBoard();
    const controller = new AbortController();
    backend.latency = 30;
    const givenUp = rejection(
      backend.upload(boardId, new File(['x'], 'slow.txt'), { signal: controller.signal }),
    );
    controller.abort();
    backend.failNext('upload', new BackendError(413, { limit: 1 }));
    // The one given up ends as given up, although the failure was waiting when its turn came.
    expect((await givenUp).status).toBe(0);
    backend.latency = 0;
    const refused = await rejection(backend.upload(boardId, new File(['y'], 'next.txt')));
    expect(refused.status).toBe(413);
  });
});

describe('the seed made of the built-in templates', () => {
  it('holds every template as a board the reader is on, with documents that pass', async () => {
    const templates = await loadTemplates();
    expect(templates).toHaveLength(13);
    const { backend, world } = setup(templates);
    const [team] = await backend.teams();
    const boards = await backend.boards(team?.id ?? '');
    expect(boards).toHaveLength(13);
    const cards = (await Promise.all(boards.map((board) => backend.cards(board.id)))).flat();
    expect(cards).toHaveLength(77);
    expect(cards.filter((card) => card.hasBody)).toHaveLength(58);
    expect(new Set([...world.blocks.keys(), ...world.boards.keys()]).size).toBe(
      world.blocks.size + world.boards.size,
    );
    const categories = await backend.categories(team?.id ?? '');
    expect(categories.flatMap((category) => category.boardIds).sort()).toEqual(
      boards.map((board) => board.id).sort(),
    );
  });

  it('comes out the same every time', async () => {
    const templates = await loadTemplates();
    const dump = (): string => {
      const { world } = setup(templates);
      return JSON.stringify([[...world.boards], [...world.blocks], [...world.categories]]);
    };
    expect(dump()).toBe(dump());
  });
});
