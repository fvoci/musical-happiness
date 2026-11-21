/**
 * Each account's own space in the backend in memory, as the contract has it
 * ("The user's own space"): listed last with its kind and no name, a
 * workspace like another to what is under it, with its owner alone in it.
 * Of anybody else's it is not there (404), open boards and wikis included,
 * and nothing goes between it and the shared workspace but by archive.
 */
import { describe, expect, it } from 'vitest';

import { firstKanban } from '$lib/state/workspace-entry.ts';
import { BackendError, passwordSet, type Change } from '$lib/state/index.ts';
import type { MemoryBackend } from '$lib/state/memory/backend.ts';

import { setup } from './support.ts';

async function statusOf(promise: Promise<unknown>): Promise<number | 'ok'> {
  try {
    await promise;
    return 'ok';
  } catch (error) {
    if (error instanceof BackendError) return error.status;
    throw error;
  }
}

async function ownSpace(backend: MemoryBackend): Promise<string> {
  const own = (await backend.workspaces()).find((workspace) => workspace.kind === 'personal');
  if (!own) throw new Error('no own space');
  return own.id;
}

async function boardIn(backend: MemoryBackend, workspaceId: string, title: string, open = false) {
  const made = await backend.createBoard(
    workspaceId,
    { title, ...(open ? { type: 'O' as const } : {}) },
    firstKanban([], 'Board'),
  );
  return made.board;
}

describe('the own space in memory', () => {
  it('is listed last, of its kind and without a name, one for each account', async () => {
    const { backend, other, world } = setup();
    const mine = await backend.workspaces();
    expect(mine.map((workspace) => workspace.kind)).toEqual([world.workspace.kind, 'personal']);
    expect(world.workspace.kind).not.toBe('personal');
    expect(mine[0]?.id).toBe(world.workspace.id);
    expect(mine[1]?.title).toBe('');
    expect(await backend.workspaces()).toEqual(mine);
    const theirs = await other.workspaces();
    expect(theirs[1]?.id).not.toBe(mine[1]?.id);
  });

  it('lists what is made in it there alone, filed under its own default category', async () => {
    const { backend, world } = setup();
    const own = await ownSpace(backend);
    const told: Change[] = [];
    backend.watchSelf((change) => told.push(change));
    const board = await boardIn(backend, own, 'Mine');
    const wiki = await backend.wiki.createWiki(own, { title: 'Notes', type: 'P' });
    expect((await backend.boards(own)).map((one) => one.id)).toEqual([board.id]);
    expect((await backend.wiki.wikis(own)).map((one) => one.id)).toEqual([wiki.id]);
    expect((await backend.boards(world.workspace.id)).map((one) => one.id)).not.toContain(board.id);
    const categories = await backend.categories(own);
    expect(categories).toHaveLength(1);
    expect(categories[0]?.type).toBe('system');
    expect(categories[0]?.containerIds).toEqual([board.id, wiki.id]);
    expect(
      (await backend.categories(world.workspace.id)).flatMap((one) => one.containerIds),
    ).not.toContain(board.id);
    // What the sidebar is told is the categories of one workspace.
    for (const change of told) {
      if (change.kind !== 'categories') continue;
      expect(change.categories.every((one) => one.workspaceId === own)).toBe(true);
    }
  });

  it('has its owner alone in it', async () => {
    const { backend } = setup();
    const own = await ownSpace(backend);
    const me = await backend.me();
    expect((await backend.users(own)).map((user) => user.id)).toEqual([me.id]);
    expect((await backend.searchUsers(own, '')).map((user) => user.id)).toEqual([me.id]);
    const board = await boardIn(backend, own, 'Mine');
    const someone = (await backend.users((await backend.workspaces())[0]?.id ?? '')).find(
      (user) => user.id !== me.id,
    );
    if (!someone) throw new Error('the seed has one person only');
    expect(await statusOf(backend.addMember(board.id, someone.id, 'editor'))).toBe(400);
    expect(await backend.usersById(own, [someone.id])).toEqual([]);
  });

  it('is not there for anybody else, its open boards and wikis included', async () => {
    const { backend, other } = setup();
    const own = await ownSpace(backend);
    const board = await boardIn(backend, own, 'Open', true);
    const wiki = await backend.wiki.createWiki(own, { title: 'Open notes', type: 'O' });
    for (const asked of [
      other.boards(own),
      other.users(own),
      other.usersById(own, []),
      other.categories(own),
      other.templates(own),
      other.searchBoards(own, 'Open'),
      other.deletedContainers(own),
      other.createBoard(own, { title: 'In' }, firstKanban([], 'Board')),
      other.onboard(own),
      other.wiki.wikis(own),
      other.wiki.createWiki(own, { title: 'In', type: 'P' }),
      other.search.find(own, { query: 'Open' }),
      other.references.resolve(own, [board.id]),
      other.exportWorkspace(own),
      other.joinContainer(board.id),
      other.views(board.id),
      other.cards(board.id),
      other.wiki.pages(wiki.id),
    ]) {
      expect(await statusOf(asked)).toBe(404);
    }
  });

  it('takes nothing from the shared workspace but a built-in template, and gives nothing to it', async () => {
    const { backend, world } = setup();
    const own = await ownSpace(backend);
    const shared = (await backend.boards(world.workspace.id))[0];
    if (!shared) throw new Error('no shared board');
    expect(await statusOf(backend.duplicateContainer(shared.id, false, own))).toBe(404);
    const mine = await boardIn(backend, own, 'Mine');
    expect(await statusOf(backend.duplicateContainer(mine.id, false, world.workspace.id))).toBe(
      404,
    );
    const copied = await backend.duplicateContainer(mine.id, false, own);
    expect(copied.workspaceId).toBe(own);
    // A built-in template of the server, the welcome board among them, goes into any workspace.
    const builtIn = world.containers.get(shared.id);
    if (!builtIn) throw new Error('no shared board');
    Object.assign(builtIn, { isTemplate: true, isBuiltin: true, properties: { onboarding: '1' } });
    expect((await backend.templates(own)).map((one) => one.id)).toEqual([shared.id]);
    expect((await backend.duplicateContainer(shared.id, false, own)).workspaceId).toBe(own);
    const welcome = await backend.onboard(own);
    expect((await backend.container(welcome)).workspaceId).toBe(own);
  });

  it('brings a deleted board back into its own category, and tells that workspace’s', async () => {
    const { backend } = setup();
    const own = await ownSpace(backend);
    const board = await boardIn(backend, own, 'Back');
    await backend.deleteContainer(board.id);
    expect((await backend.deletedContainers(own)).map((item) => item.id)).toEqual([board.id]);
    const told: Change[] = [];
    backend.watchSelf((change) => told.push(change));
    await backend.restoreContainer(board.id);
    expect((await backend.categories(own))[0]?.containerIds).toEqual([board.id]);
    const categories = told.flatMap((change) =>
      change.kind === 'categories' ? change.categories : [],
    );
    expect(categories.length).toBeGreaterThan(0);
    expect(categories.every((one) => one.workspaceId === own)).toBe(true);
  });

  it('links to nothing across spaces: a card of the shared workspace is not found from it', async () => {
    const { backend, world } = setup();
    const own = await ownSpace(backend);
    const shared = await boardIn(backend, world.workspace.id, 'Shared');
    const card = await backend.createCard(shared.id, { title: 'Crossing card' });
    const found = await backend.references.search(world.workspace.id, 'Crossing');
    expect(found.map((one) => one.id)).toEqual([card.id]);
    expect(await backend.references.search(own, 'Crossing')).toEqual([]);
    expect(await backend.references.resolve(own, [card.id])).toEqual([]);
    expect(await statusOf(backend.references.to(own, card.id))).toBe(404);
  });
});

describe('the own space, once it takes the password alone', () => {
  it('is not entered by the list that says so: nothing of it is read, and the session goes on', async () => {
    const { backend, world, workspace } = setup();
    const own = await ownSpace(backend);
    const kept = world.ownSpaces.get(backend.userId);
    if (!kept) throw new Error('no own space');
    kept.passwordRequired = true;
    await workspace.load(own);
    expect(workspace.status).toBe('ready');
    expect(workspace.entered).toBe('password');
    expect(workspace.closed?.id).toBe(own);
    expect(workspace.info).toBeUndefined();
    expect(workspace.containers).toEqual([]);
    // No people of it are read: the reader is named by their own reading.
    const reader = world.users.get(backend.userId);
    expect(workspace.userName(backend.userId)).toContain(`@${reader?.username ?? ''}`);
  });

  it('closes when the backend refuses it to the session, which is not over', async () => {
    const { backend, world, workspace } = setup();
    const own = await ownSpace(backend);
    await boardIn(backend, own, 'Mine');
    await workspace.load(own);
    expect(workspace.entered).toBe('ready');
    const kept = world.ownSpaces.get(backend.userId);
    if (!kept) throw new Error('no own space');
    kept.passwordRequired = true;
    const refused = await backend.boards(own).catch((error: unknown) => error);
    expect(refused).toMatchObject({ status: 401, reason: 'password_sign_in_required' });
    workspace.report('load', refused);
    expect(workspace.status).toBe('ready');
    expect(workspace.entered).toBe('password');
    expect(workspace.containers).toEqual([]);
    expect(workspace.workspaces.find((one) => one.id === own)?.passwordRequired).toBe(true);
  });

  it('closes on the page that gave the account its first password, and the account has one', async () => {
    const { backend, world, workspace } = setup();
    const reader = world.users.get(backend.userId);
    if (!reader) throw new Error('no reader');
    world.users.set(reader.id, { ...reader, hasPassword: false });
    const own = await ownSpace(backend);
    await workspace.load(own);
    passwordSet(workspace);
    expect(workspace.me?.hasPassword).toBe(true);
    expect(workspace.entered).toBe('password');
    expect(workspace.closed?.id).toBe(own);
    // The shared workspace is as it was.
    expect(
      workspace.workspaces.find((one) => one.id === world.workspace.id)?.passwordRequired,
    ).toBe(undefined);
  });
});
