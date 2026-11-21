/**
 * The diaries of the reader's own space and their entries on the server
 * (`Backend.diary`): the operations under `/w/{workspaceId}/diaries`
 * (`getDiaries`, `createDiary`, `getDiaryEntriesOn`, `createDiaryEntry`,
 * `getDiaryEntry`, `patchDiaryEntry`, `deleteDiaryEntry`). What a diary shares
 * with a board (reading it, renaming, deleting and bringing it back, the
 * sidebar, its archive) goes by the board's operations.
 */
import type { CalendarDate } from '../logic/dates.ts';
import type { MonthDay } from '../logic/diary-days.ts';
import type { Container } from '../logic/wire.ts';
import type {
  Diaries,
  DiaryEntry,
  DiaryEntryDetail,
  EntryPatch,
  Mood,
} from '../state/diary-types.ts';

import { bodyOf, containerOf, wireBody, type Schemas } from './convert.ts';
import type { WikiRequest } from './wikis.ts';

export function entryDetailOf(entry: Schemas['DiaryEntry']): DiaryEntryDetail {
  return {
    id: entry.id,
    diaryId: entry.diaryId,
    date: entry.date as CalendarDate,
    mood: entry.mood ?? null,
    weather: entry.weather,
    place: entry.place,
    body: bodyOf(entry.body),
    createdBy: entry.createdBy,
    modifiedBy: entry.modifiedBy,
    createAt: entry.createAt,
    updateAt: entry.updateAt,
  };
}

const MOODS: readonly string[] = ['great', 'good', 'okay', 'bad', 'awful'] satisfies Mood[];

/**
 * An entry as a change message brings it: a block of the type `entry`, whose
 * day, mood, weather and place are among its `fields`.
 */
export function entryOfBlock(block: Schemas['Block']): DiaryEntry {
  const fields = block.fields as Record<string, unknown>;
  const text = (value: unknown): string => (typeof value === 'string' ? value : '');
  const date = typeof fields.date === 'string' ? fields.date : block.title;
  return {
    id: block.id,
    diaryId: block.containerId,
    date: date as CalendarDate,
    mood: MOODS.includes(text(fields.mood)) ? (fields.mood as Mood) : null,
    weather: text(fields.weather),
    place: text(fields.place),
    createAt: block.createAt,
    updateAt: block.updateAt,
  };
}

function entryPatchOf(patch: EntryPatch): Schemas['DiaryEntryPatch'] {
  return {
    ...(patch.body === undefined ? {} : { body: patch.body && wireBody(patch.body) }),
    ...(patch.readAt === undefined ? {} : { readAt: patch.readAt }),
  };
}

export class ServerDiaries implements Diaries {
  /** A request under the workspace of the diary `params.diaryId` names (`WikiRequest`'s way). */
  readonly #request: WikiRequest;
  /** Keeps the workspace of each diary read, for the paths under it. */
  readonly #learn: (diaries: readonly Schemas['Container'][]) => void;

  constructor(request: WikiRequest, learn: (diaries: readonly Schemas['Container'][]) => void) {
    this.#request = request;
    this.#learn = learn;
  }

  async diaries(workspaceId: string): Promise<Container[]> {
    const diaries = await this.#request<Schemas['Container'][]>('/diaries', {}, {}, workspaceId);
    this.#learn(diaries);
    return diaries.map(containerOf);
  }

  async createDiary(
    workspaceId: string,
    draft: { title?: string; ifNone?: boolean },
  ): Promise<Container> {
    const request: Schemas['NewDiary'] = {
      ...(draft.title ? { title: draft.title } : {}),
      ...(draft.ifNone ? { ifNone: true } : {}),
    };
    const diary = await this.#request<Schemas['Container']>(
      '/diaries',
      {},
      { method: 'POST', json: request },
      workspaceId,
    );
    this.#learn([diary]);
    return containerOf(diary);
  }

  async entriesOn(diaryId: string, monthDay: MonthDay): Promise<DiaryEntryDetail[]> {
    const entries = await this.#request<Schemas['DiaryEntry'][]>(
      '/diaries/{diaryId}/days/{monthDay}',
      { diaryId, monthDay },
    );
    return entries.map(entryDetailOf);
  }

  async createEntry(diaryId: string, date: CalendarDate): Promise<DiaryEntryDetail> {
    const request: Schemas['NewDiaryEntry'] = { date };
    return entryDetailOf(
      await this.#request<Schemas['DiaryEntry']>(
        '/diaries/{diaryId}/entries',
        { diaryId },
        { method: 'POST', json: request },
      ),
    );
  }

  async entry(diaryId: string, entryId: string): Promise<DiaryEntryDetail> {
    return entryDetailOf(
      await this.#request<Schemas['DiaryEntry']>('/diaries/{diaryId}/entries/{entryId}', {
        diaryId,
        entryId,
      }),
    );
  }

  async patchEntry(diaryId: string, entryId: string, patch: EntryPatch): Promise<DiaryEntryDetail> {
    return entryDetailOf(
      await this.#request<Schemas['DiaryEntry']>(
        '/diaries/{diaryId}/entries/{entryId}',
        { diaryId, entryId },
        { method: 'PATCH', json: entryPatchOf(patch) },
      ),
    );
  }

  async deleteEntry(diaryId: string, entryId: string): Promise<void> {
    await this.#request(
      '/diaries/{diaryId}/entries/{entryId}',
      { diaryId, entryId },
      { method: 'DELETE' },
    );
  }
}
