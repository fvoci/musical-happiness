/**
 * The search of what cards and pages say, at the data boundary: the backend
 * in memory finds as the server does (what the reader has a role on, no
 * templates, a snippet in stretches), the server's is asked as the contract
 * says, and a reader by a public link has none.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { SharedBackend } from '$lib/api/shared-backend.ts';
import { createBoard, createCard, type Block } from '$lib/logic/wire.ts';
import { BackendError, type SearchHit } from '$lib/state/backend.ts';
import { MemoryBackend, memberOf } from '$lib/state/memory/backend.ts';
import { markedParts, SNIPPET_CHARS, snippetOf } from '$lib/state/memory/content-search.ts';
import { World } from '$lib/state/memory/world.ts';

import { serverBackend, text } from './support.ts';

const NOW = Date.UTC(2026, 10, 15, 3);

function block(id: string, boardId: string, title: string, updateAt = NOW): Block {
  return {
    id,
    type: 'card',
    boardId,
    parentId: boardId,
    createdBy: 'reader',
    modifiedBy: 'reader',
    schema: 1,
    title,
    fields: {},
    createAt: NOW,
    updateAt,
    deleteAt: 0,
  };
}

/** A world of boards: the reader's, another's, an open one not joined, and a template. */
function world(): World {
  const made = new World(() => NOW);
  made.workspace = { id: 'w', title: 'Example', role: 'admin' };
  const board = (id: string, extra: object = {}): void => {
    made.boards.set(id, { ...createBoard(), id, workspaceId: 'w', title: `Board ${id}`, ...extra });
  };
  board('mine');
  board('theirs');
  board('open', { type: 'O' });
  board('template', { isTemplate: true });
  made.members.set('mine', [memberOf('mine', 'reader', 'viewer')]);
  made.members.set('theirs', [memberOf('theirs', 'someone', 'admin')]);
  made.members.set('open', [memberOf('open', 'someone', 'admin')]);
  made.members.set('template', [memberOf('template', 'reader', 'admin')]);
  const card = (id: string, boardId: string, title: string, body: string, extra = {}): void => {
    made.blocks.set(id, {
      ...createCard(block(id, boardId, title)),
      body: text(body),
      ...extra,
    });
  };
  card('found', 'mine', 'Release plan', 'The kangaroo route is ready');
  card('titled', 'mine', 'Kangaroo', 'Nothing about it here');
  card('secret', 'theirs', 'Theirs', 'A kangaroo of theirs');
  card('unjoined', 'open', 'Open', 'A kangaroo nobody joined');
  card('in-template', 'template', 'Template', 'A kangaroo template');
  const template = createCard(block('card-template', 'mine', 'Card template'));
  made.blocks.set('card-template', {
    ...template,
    fields: { ...template.fields, isTemplate: true },
    body: text('A kangaroo to copy'),
  });
  return made;
}

const ids = (hits: SearchHit[]): string[] => hits.map((hit) => hit.id);

describe('the search in memory', () => {
  it('finds the cards of the boards the reader has a role on, of no template', async () => {
    const backend = new MemoryBackend(world(), 'reader');
    const hits = await backend.search.find('w', { query: 'kangaroo' });
    // The title first.
    expect(ids(hits)).toEqual(['titled', 'found']);
    expect(hits[1]).toMatchObject({
      kind: 'card',
      containerId: 'mine',
      containerKind: 'board',
      title: 'Release plan',
    });
  });

  it('finds the pages of an open wiki the reader works beside, not joined, as the server does', async () => {
    const made = world();
    made.boards.set('notes', {
      ...createBoard(),
      id: 'notes',
      workspaceId: 'w',
      title: 'Notes',
      type: 'O',
      kind: 'wiki',
    });
    made.members.set('notes', [memberOf('notes', 'someone', 'admin')]);
    made.blocks.set('note', { ...block('note', 'notes', 'Kangaroo notes'), type: 'page' });
    const backend = new MemoryBackend(made, 'reader');
    expect(ids(await backend.search.find('w', { query: 'notes' }))).toEqual([]);
    made.workspaceMembers.add('reader');
    const hits = await backend.search.find('w', { query: 'notes' });
    expect(hits.map((hit) => [hit.id, hit.containerKind])).toEqual([['note', 'wiki']]);
  });

  it('wants every word, in any case, and finds nothing for no words', async () => {
    const backend = new MemoryBackend(world(), 'reader');
    expect(ids(await backend.search.find('w', { query: 'ROUTE kangaroo' }))).toEqual(['found']);
    expect(ids(await backend.search.find('w', { query: 'route wallaby' }))).toEqual([]);
    expect(await backend.search.find('w', { query: '   ' })).toEqual([]);
  });

  it('finds one kind when asked, and as many as asked', async () => {
    const backend = new MemoryBackend(world(), 'reader');
    expect(await backend.search.find('w', { query: 'kangaroo', kind: 'page' })).toEqual([]);
    expect(await backend.search.find('w', { query: 'kangaroo', limit: 1 })).toHaveLength(1);
  });

  it('marks the words in a piece of the body, and gives the beginning of a body that has none', async () => {
    const backend = new MemoryBackend(world(), 'reader');
    const [titled, found] = await backend.search.find('w', { query: 'kangaroo' });
    expect(found?.snippet).toEqual([
      { text: 'The ', matched: false },
      { text: 'kangaroo', matched: true },
      { text: ' route is ready', matched: false },
    ]);
    expect(titled?.snippet).toEqual([{ text: 'Nothing about it here', matched: false }]);
  });

  it('refuses what the server refuses', async () => {
    const backend = new MemoryBackend(world(), 'reader');
    const refusal = (promise: Promise<unknown>): Promise<unknown> =>
      promise.catch((error: unknown) => error);
    const long = await refusal(backend.search.find('w', { query: 'x'.repeat(201) }));
    expect(long).toBeInstanceOf(BackendError);
    expect((long as BackendError).status).toBe(400);
    expect((long as BackendError).errors[0]?.parameter).toBe('q');
    // Characters, not code units: two hundred of a script beyond the first plane pass.
    await expect(backend.search.find('w', { query: '𝒳'.repeat(200) })).resolves.toEqual([]);
    const limit = await refusal(backend.search.find('w', { query: 'a', limit: 51 }));
    expect((limit as BackendError).status).toBe(400);
    const elsewhere = await refusal(backend.search.find('other', { query: 'a' }));
    expect((elsewhere as BackendError).status).toBe(404);
  });

  it('is off where the world has no search server, and says so as the server does', async () => {
    const off = world();
    off.searchEnabled = false;
    const backend = new MemoryBackend(off, 'reader');
    expect(await backend.search.enabled()).toBe(false);
    await expect(backend.search.find('w', { query: 'kangaroo' })).rejects.toMatchObject({
      status: 404,
      reason: 'search_not_configured',
    });
    expect(await new MemoryBackend(world(), 'reader').search.enabled()).toBe(true);
  });

  it('takes the failure a test asked for, as every call does', async () => {
    const backend = new MemoryBackend(world(), 'reader');
    backend.failNext('search', new BackendError(503, { reason: 'search_unavailable' }));
    await expect(backend.search.find('w', { query: 'kangaroo' })).rejects.toMatchObject({
      status: 503,
    });
    await expect(backend.search.find('w', { query: 'kangaroo' })).resolves.toHaveLength(2);
  });
});

describe('the snippet of a body', () => {
  it('is cut around the first word found, in characters', () => {
    const body = `${'가'.repeat(300)} target ${'나'.repeat(300)}`;
    const parts = snippetOf(body, ['target']);
    const joined = parts.map((part) => part.text).join('');
    expect(Array.from(joined)).toHaveLength(SNIPPET_CHARS);
    expect(parts.filter((part) => part.matched).map((part) => part.text)).toEqual(['target']);
  });

  it('marks the longest of two words where one holds the other, and keeps the case of the text', () => {
    expect(markedParts('Foobar and foo', ['foo', 'FOOBAR'])).toEqual([
      { text: 'Foobar', matched: true },
      { text: ' and ', matched: false },
      { text: 'foo', matched: true },
    ]);
  });

  it('is not moved by letters whose small form is longer', () => {
    const body = `${'İ'.repeat(100)} middle target ${'x'.repeat(300)}`;
    const joined = snippetOf(body, ['target'])
      .map((part) => part.text)
      .join('');
    // Forty characters before the word, as for any other letters.
    expect(joined.startsWith(`${'İ'.repeat(32)} middle target`)).toBe(true);
  });

  it('is empty for an empty body', () => {
    expect(snippetOf('', ['a'])).toEqual([]);
  });
});

describe('the search on the server', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function serve(answers: Record<string, unknown>): string[] {
    const asked: string[] = [];
    vi.stubGlobal('fetch', (url: string) => {
      asked.push(url);
      const path = new URL(url, 'http://here').pathname;
      return Promise.resolve(
        new Response(JSON.stringify(answers[path] ?? {}), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      );
    });
    return asked;
  }

  it('asks the search of the workspace, and gives its hits as they are', async () => {
    const hit = {
      id: 'c',
      kind: 'card',
      containerId: 'b',
      containerKind: 'board',
      title: 'Title',
      snippet: [{ text: '<b>', matched: true }],
    };
    const asked = serve({ '/api/v1/w/w1/search': [hit] });
    const backend = serverBackend('');
    const hits = await backend.search.find('w1', { query: '환불 규정', kind: 'card', limit: 20 });
    expect(hits).toEqual([hit]);
    const url = new URL(asked[0] ?? '', 'http://here');
    expect(url.searchParams.get('q')).toBe('환불 규정');
    expect(url.searchParams.get('kind')).toBe('card');
    expect(url.searchParams.get('limit')).toBe('20');
    // Without a kind the server finds both.
    await backend.search.find('w1', { query: 'a' });
    expect(new URL(asked[1] ?? '', 'http://here').searchParams.has('kind')).toBe(false);
  });

  it('is on as the settings for clients say, which are read once', async () => {
    const config = {
      enablePublicSharedBoards: true,
      featureFlags: {},
      maxFileSize: 1,
      maxImportSize: 1,
      searchEnabled: true,
      teammateNameDisplay: 'username',
    };
    const asked = serve({ '/api/v1/client-config': config });
    const backend = serverBackend('');
    expect(await backend.search.enabled()).toBe(true);
    expect(await backend.search.enabled()).toBe(true);
    expect(asked.filter((url) => url.endsWith('/client-config'))).toHaveLength(1);
  });
});

describe('the search of a reader by a public link', () => {
  it('is off, and finds nothing without asking', async () => {
    const fetched = vi.fn();
    vi.stubGlobal('fetch', fetched);
    const backend = new SharedBackend('', 'board', 'token');
    expect(await backend.search.enabled()).toBe(false);
    expect(await backend.search.find('w', { query: 'a' })).toEqual([]);
    expect(fetched).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
