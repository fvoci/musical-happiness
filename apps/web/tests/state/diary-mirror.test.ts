/**
 * The backend in memory keeps the server's rules where a diary is reached by
 * a way other than its own routes: an archive imported, a diary brought back
 * from the trash, search, and the routes of boards and wikis (members,
 * sharing, a board's blocks), which have no diary. And the page of a diary
 * keeps its days true to what the diary has after what it missed (a resync),
 * and when a day left blank is opened again before its entry is deleted.
 */
import { describe, expect, it, vi } from 'vitest';

import { dateOfUtc } from '$lib/logic/dates.ts';
import { monthDayOf } from '$lib/logic/diary-days.ts';
import { BackendError, DiaryStore, firstDiary, type Change } from '$lib/state/index.ts';
import type { MemoryBackend } from '$lib/state/memory/backend.ts';
import { DIARIES_PER_SPACE } from '$lib/state/memory/diaries.ts';
// Loaded before a room is opened, as the page has it by then.
import '$lib/state/memory/rooms.ts';

import { settle, setup, text } from './support.ts';

async function statusOf(promise: Promise<unknown>): Promise<number | 'ok'> {
  try {
    await promise;
    return 'ok';
  } catch (error) {
    if (error instanceof BackendError) return error.status;
    throw error;
  }
}

async function spaces(backend: MemoryBackend): Promise<{ ownId: string; sharedId: string }> {
  const all = await backend.workspaces();
  const own = all.find((workspace) => workspace.kind === 'personal');
  const shared = all.find((workspace) => workspace.kind !== 'personal');
  if (!own || !shared) throw new Error('no own space, or no shared one');
  return { ownId: own.id, sharedId: shared.id };
}

/** A diary with an entry, and its archive as `exportContainer` makes it. */
async function anArchive() {
  const made = setup();
  const { ownId, sharedId } = await spaces(made.backend);
  const diaries = made.backend.diary;
  const diary = await diaries.createDiary(ownId, { title: 'Days' });
  const today = dateOfUtc(made.world.now());
  const entry = await diaries.createEntry(diary.id, today);
  const saved = await made.backend.exportContainer(diary.id);
  const lines = (await saved.bytes.text()).split('\n').filter((line) => line !== '');
  return { ...made, ownId, sharedId, diaries, diary, entry, today, lines, name: saved.name };
}

/** The archive of `lines`, each changed as `change` has it. */
function archiveOf(lines: readonly string[], name: string, more: readonly string[] = []): File {
  return new File([[...lines, ...more].join('\n')], name);
}

/** The line of the archive that holds the entry, its data changed by `change`. */
function entryLine(
  lines: readonly string[],
  change: (data: Record<string, unknown>) => void,
): string {
  const line = lines.find((one) => one.includes('"entry"'));
  if (!line) throw new Error('the archive holds no entry');
  const parsed = JSON.parse(line) as { data: Record<string, unknown> };
  change(parsed.data);
  return JSON.stringify(parsed);
}

describe('a diary reached by another way than its own routes, in memory', () => {
  it('is imported into the own space alone, as many as it may hold', async () => {
    const made = await anArchive();
    const into = (workspaceId: string) =>
      statusOf(made.backend.importArchive(workspaceId, archiveOf(made.lines, made.name)));
    expect(await into(made.sharedId)).toBe(400);
    expect(await into(made.ownId)).toBe('ok');
    expect(await made.diaries.diaries(made.ownId)).toHaveLength(2);
    for (let left = DIARIES_PER_SPACE - 2; left > 0; left -= 1) {
      await made.diaries.createDiary(made.ownId, {});
    }
    expect(await into(made.ownId)).toBe(400);
    expect(await made.diaries.diaries(made.ownId)).toHaveLength(DIARIES_PER_SPACE);
  });

  it('is not imported with a day twice, or one far ahead', async () => {
    const made = await anArchive();
    const twice = entryLine(made.lines, (data) => {
      data.id = 'another-entry';
    });
    const ahead = entryLine(made.lines, (data) => {
      data.title = '2999-01-01';
      data.fields = { ...(data.fields as object), date: '2999-01-01' };
    });
    const asIs = made.lines.filter((line) => !line.includes('"entry"'));
    for (const archive of [
      archiveOf(made.lines, made.name, [twice]),
      archiveOf(asIs, made.name, [ahead]),
    ]) {
      expect(await statusOf(made.backend.importArchive(made.ownId, archive))).toBe(400);
    }
    // Refused before anything was stored.
    expect(await made.diaries.diaries(made.ownId)).toHaveLength(1);
  });

  it('comes back from the trash only where there is room for it', async () => {
    const made = await anArchive();
    await made.backend.deleteContainer(made.diary.id);
    for (let left = DIARIES_PER_SPACE; left > 0; left -= 1) {
      await made.diaries.createDiary(made.ownId, {});
    }
    expect(await statusOf(made.backend.restoreContainer(made.diary.id))).toBe(400);
  });

  it('is searched: its entries are found as entries of a diary', async () => {
    const made = await anArchive();
    await made.diaries.patchEntry(made.diary.id, made.entry.id, {
      body: text('바다에 간 날'),
      readAt: made.entry.updateAt,
    });
    const hits = await made.backend.search.find(made.ownId, { query: '바다에' });
    expect(hits).toEqual([
      expect.objectContaining({ id: made.entry.id, kind: 'entry', containerKind: 'diary' }),
    ]);
  });

  it('has no members, no sharing and no blocks of a board, and its entries are below no block', async () => {
    const made = await anArchive();
    const { backend, diary, entry } = made;
    const reader = backend.userId;
    for (const asked of [
      backend.members(diary.id),
      backend.addMember(diary.id, reader, 'editor'),
      backend.setMemberRole(diary.id, reader, 'editor'),
      backend.removeMember(diary.id, reader),
      backend.sharing(diary.id),
      backend.setSharing(diary.id, { enabled: true }),
      backend.deleteBlock(diary.id, entry.id),
    ]) {
      expect(await statusOf(asked)).toBe(404);
    }
    expect(made.world.blocks.get(entry.id)?.parentId).toBe('');
  });
});

/** The reader's diary page on today, with what it hears passed through `heard`. */
async function aPage(heard: (change: Change) => boolean = () => true) {
  const made = setup();
  const { ownId } = await spaces(made.backend);
  await made.workspace.load(ownId);
  const diary = await firstDiary(made.workspace);
  if (!diary) throw new Error('no diary');
  let tell: (change: Change) => void = () => undefined;
  const watch = made.backend.watchContainer.bind(made.backend);
  vi.spyOn(made.backend, 'watchContainer').mockImplementation((id, listener) => {
    tell = listener;
    return watch(id, (change) => {
      if (heard(change)) listener(change);
    });
  });
  const store = new DiaryStore(made.workspace, diary.id);
  const today = dateOfUtc(made.world.now());
  await store.show(monthDayOf(today));
  return {
    ...made,
    diary,
    store,
    today,
    tell: (change: Change) => {
      tell(change);
    },
  };
}

describe('a diary’s page after what it missed', () => {
  it('lets a day go whose entry was deleted unheard, once what was missed is read', async () => {
    // Deletions are not heard: the page is offline as the entry goes.
    const { store, backend, diary, today, tell } = await aPage(
      (change) => change.kind !== 'blocksDeleted',
    );
    const there = await backend.diary.createEntry(diary.id, today);
    await store.show(monthDayOf(today));
    const entry = store.day(today);
    await vi.waitFor(() => {
      expect(entry.entry?.id).toBe(there.id);
    });
    await backend.as(backend.userId).diary.deleteEntry(diary.id, there.id);
    await settle();
    expect(entry.entry?.id).toBe(there.id);
    tell({ kind: 'resync' });
    await vi.waitFor(() => {
      expect(entry.entry).toBeUndefined();
    });
    expect(store.entries).toEqual([]);
    store.close();
  });

  it('opens the room of the entry the day has now, made elsewhere after one deleted unheard', async () => {
    const { store, backend, diary, today } = await aPage(
      (change) => change.kind !== 'blocksDeleted',
    );
    const there = await backend.diary.createEntry(diary.id, today);
    await store.show(monthDayOf(today));
    const entry = store.day(today);
    entry.openBody();
    await vi.waitFor(() => {
      expect(entry.room?.status).toBe('joined');
    });
    const opening = vi.spyOn(backend, 'openDoc');
    const elsewhere = backend.as(backend.userId).diary;
    await elsewhere.deleteEntry(diary.id, there.id);
    const again = await elsewhere.createEntry(diary.id, today);
    await vi.waitFor(() => {
      expect(entry.entry?.id).toBe(again.id);
    });
    expect(opening).toHaveBeenCalledWith(expect.anything(), diary.id, again.id, expect.anything());
    store.close();
  });

  it('keeps an entry left blank and opened again before it was deleted', async () => {
    const { store, backend, diary, today } = await aPage();
    const entry = store.day(today);
    entry.openBody();
    entry.mark({ mood: 'bad' });
    await vi.waitFor(() => {
      expect(entry.room?.status).toBe('joined');
    });
    entry.mark({ mood: null });
    entry.noteBlank(true);
    entry.closeBody();
    // Opened again while what the room sends is settled.
    entry.openBody();
    await settle();
    await vi.waitFor(() => {
      expect(entry.room?.status).toBe('joined');
    });
    expect(await backend.diary.entriesOn(diary.id, monthDayOf(today))).toHaveLength(1);
    store.close();
  });
});
