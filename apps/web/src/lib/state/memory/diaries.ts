/**
 * The diaries of the backend in memory (`Backend.diary`), as the server keeps
 * them (`crates/store/src/diaries.rs`): a diary is a board of the kind
 * `diary`, private, its maker its only member, in their own space and
 * nowhere else (any other workspace is not there for its operations: 404); a
 * space has at most `DIARIES_PER_SPACE`. Its entries are blocks of the type
 * `entry`, titled with their day, which is among their `fields` with their
 * mood, weather and place; a diary has one entry a day (making a day's entry
 * again answers the one there) and at most `ENTRIES_PER_DIARY`. A day is a
 * day of the calendar no more than two days past the world's own (UTC).
 * Deleting an entry puts it in the trash, where it holds no day.
 */
import { addDays, dateOfUtc, parseCalendarDate, type CalendarDate } from '../../logic/dates.ts';
import { monthDayOf, type MonthDay } from '../../logic/diary-days.ts';
import { createBoard, type Container } from '../../logic/wire.ts';
import type { Diaries, DiaryEntryDetail, EntryPatch } from '../diary-types.ts';

import type { BackendError } from '../problems.ts';

import { checkedWhole } from './bodies.ts';
import { fileAsNew } from './categories.ts';
import { pause } from './pause.ts';
import { fail } from './refuse.ts';
import { changeOf, copy, entryOf, memberOf } from './shapes.ts';
import { checkWorkspace, ownSpaceOf } from './spaces.ts';
import { deletion } from './trash.ts';
import type { StoredBlock, World } from './world.ts';

/** The server's limits (`crates/model/src/diary.rs`). */
export const DIARIES_PER_SPACE = 20;
export const ENTRIES_PER_DIARY = 40_000;
const DAYS_AHEAD = 2;

/** What the diaries share with the backend they are of. */
export interface DiaryHost {
  readonly world: World;
  readonly userId: string;
  /** Milliseconds every call waits before it answers. */
  readonly latency: number;
}

const isDiary = (board: Container | undefined): board is Container => board?.kind === 'diary';

function detailOf(block: StoredBlock): DiaryEntryDetail {
  return {
    ...entryOf(block),
    body: copy(block.body ?? null),
    createdBy: block.createdBy,
    modifiedBy: block.modifiedBy,
  };
}

export class MemoryDiaries implements Diaries {
  readonly #host: DiaryHost;
  readonly #failures = new Map<keyof Diaries, BackendError>();

  constructor(host: DiaryHost) {
    this.#host = host;
  }

  /** Makes the next call of `method` reject with `error`, once. */
  failNext(method: keyof Diaries, error: BackendError): void {
    this.#failures.set(method, error);
  }

  async #enter(method: keyof Diaries): Promise<void> {
    await pause(this.#host.latency);
    const failure = this.#failures.get(method);
    if (failure) {
      this.#failures.delete(method);
      throw failure;
    }
  }

  get #world(): World {
    return this.#host.world;
  }

  /** 404 unless the workspace is the reader's own space (which has diaries). */
  #ownSpace(workspaceId: string): void {
    checkWorkspace(this.#world, this.#host.userId, workspaceId);
    if (ownSpaceOf(this.#world, this.#host.userId).id !== workspaceId) {
      fail(404, 'no diaries in this workspace');
    }
  }

  #diariesIn(workspaceId: string): Container[] {
    return [...this.#world.containers.values()]
      .filter((board) => isDiary(board) && board.workspaceId === workspaceId)
      .sort((first, second) => first.createAt - second.createAt);
  }

  /** The reader's diary; anything else is not there. */
  #diary(diaryId: string): Container {
    const diary = this.#world.containers.get(diaryId);
    const mine = (this.#world.members.get(diaryId) ?? []).some(
      (member) => member.userId === this.#host.userId,
    );
    if (!isDiary(diary) || !mine) fail(404, 'no such diary');
    checkWorkspace(this.#world, this.#host.userId, diary.workspaceId);
    return diary;
  }

  #entriesOf(diaryId: string): StoredBlock[] {
    return this.#world.blocksOf(diaryId).filter((block) => block.type === 'entry');
  }

  #entry(diaryId: string, entryId: string): StoredBlock {
    const block = this.#world.blocks.get(entryId);
    if (block?.type !== 'entry' || block.containerId !== diaryId) fail(404, 'no such entry');
    return block;
  }

  async diaries(workspaceId: string): Promise<Container[]> {
    await this.#enter('diaries');
    this.#ownSpace(workspaceId);
    return this.#diariesIn(workspaceId).map(copy);
  }

  async createDiary(
    workspaceId: string,
    draft: { title?: string; ifNone?: boolean },
  ): Promise<Container> {
    await this.#enter('createDiary');
    this.#ownSpace(workspaceId);
    const world = this.#world;
    const userId = this.#host.userId;
    const there = this.#diariesIn(workspaceId);
    const oldest = there[0];
    if (draft.ifNone && oldest) return copy(oldest);
    if (there.length >= DIARIES_PER_SPACE) {
      fail(400, 'the space has as many diaries as it may', {
        reason: 'too_many_diaries',
        limit: DIARIES_PER_SPACE,
      });
    }
    const now = world.now();
    const diary: Container = {
      ...createBoard(),
      id: world.newId(),
      workspaceId,
      kind: 'diary',
      createdBy: userId,
      modifiedBy: userId,
      type: 'P',
      title: draft.title ?? '',
      cardProperties: [],
      createAt: now,
      updateAt: now,
    };
    world.containers.set(diary.id, diary);
    world.members.set(diary.id, [memberOf(diary.id, userId, 'admin')]);
    world.tellUsers([userId], { kind: 'container', container: copy(diary) });
    fileAsNew(world, userId, diary);
    return copy(diary);
  }

  async entriesOn(diaryId: string, monthDay: MonthDay): Promise<DiaryEntryDetail[]> {
    await this.#enter('entriesOn');
    this.#diary(diaryId);
    return this.#entriesOf(diaryId)
      .map(detailOf)
      .filter((entry) => monthDayOf(entry.date) === monthDay)
      .sort((first, second) => second.date.localeCompare(first.date));
  }

  async createEntry(diaryId: string, date: CalendarDate): Promise<DiaryEntryDetail> {
    await this.#enter('createEntry');
    this.#diary(diaryId);
    const world = this.#world;
    const day = parseCalendarDate(date);
    if (!day) fail(400, 'no such day', { field: 'date' });
    if (day > addDays(dateOfUtc(world.now()), DAYS_AHEAD)) {
      fail(400, 'the day is too far ahead', { field: 'date' });
    }
    const entries = this.#entriesOf(diaryId);
    const there = entries.find((block) => block.title === day);
    if (there) return detailOf(there);
    if (entries.length >= ENTRIES_PER_DIARY) {
      fail(400, 'the diary has as many entries as it may', {
        reason: 'too_many_entries',
        limit: ENTRIES_PER_DIARY,
      });
    }
    const now = world.now();
    const userId = this.#host.userId;
    const block: StoredBlock = {
      id: world.newId(),
      containerId: diaryId,
      parentId: diaryId,
      createdBy: userId,
      modifiedBy: userId,
      schema: 1,
      type: 'entry',
      title: day,
      fields: { date: day, mood: null, weather: '', place: '' },
      body: null,
      createAt: now,
      updateAt: now,
      deleteAt: 0,
    };
    world.blocks.set(block.id, block);
    world.tellContainer(diaryId, changeOf(diaryId, block));
    return detailOf(block);
  }

  async entry(diaryId: string, entryId: string): Promise<DiaryEntryDetail> {
    await this.#enter('entry');
    this.#diary(diaryId);
    return detailOf(this.#entry(diaryId, entryId));
  }

  async patchEntry(diaryId: string, entryId: string, patch: EntryPatch): Promise<DiaryEntryDetail> {
    await this.#enter('patchEntry');
    this.#diary(diaryId);
    const block = this.#entry(diaryId, entryId);
    if (patch.body !== undefined && patch.readAt === undefined) {
      fail(400, 'a body is written with the version it was read at', { field: 'readAt' });
    }
    if (patch.readAt !== undefined && patch.readAt !== block.updateAt) {
      fail(409, 'the entry changed since it was read');
    }
    if (patch.body !== undefined) {
      block.body = checkedWhole(this.#world, diaryId, entryId, patch.body);
      block.updateAt = this.#world.now();
      block.modifiedBy = this.#host.userId;
      // As the server: its room's document is made anew from the body written whole.
      this.#world.bodyWritten?.(entryId);
      this.#world.tellContainer(diaryId, changeOf(diaryId, block));
    }
    return detailOf(block);
  }

  async deleteEntry(diaryId: string, entryId: string): Promise<void> {
    await this.#enter('deleteEntry');
    this.#diary(diaryId);
    const block = this.#entry(diaryId, entryId);
    this.#world.deletedBlocks.set(entryId, {
      containerId: diaryId,
      blocks: [block],
      ...deletion({ world: this.#world, userId: this.#host.userId }),
    });
    this.#world.blocks.delete(entryId);
    this.#world.tellContainer(diaryId, {
      kind: 'blocksDeleted',
      containerId: diaryId,
      blockIds: [entryId],
    });
  }
}
