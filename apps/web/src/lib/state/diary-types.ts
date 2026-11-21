/**
 * The diaries of the reader's own space and their entries, part of the data
 * boundary (`backend.ts`, `Backend.diary`).
 *
 * A diary is a board of the kind `diary` (`Container.kind`), only ever in the
 * reader's own space: it is renamed, deleted, brought back, filed in the
 * sidebar, exported and searched as a board is, by the board's operations.
 * Nobody else is on it: no members, no public link, no copies. What only a
 * diary has is here: its entries, one a day at most, each with a body
 * written together in its room as a card's is (`Backend.openDoc` with the
 * entry in place of the card).
 *
 * The day of an entry is the day where its writer was, as their device has it:
 * the server never decides what today is. It is what the entry is made with
 * and never changes.
 */
import type { DocNode } from '../editor/schema.ts';
import type { CalendarDate } from '../logic/dates.ts';
import type { MonthDay } from '../logic/diary-days.ts';
import type { Container } from '../logic/wire.ts';

/** How the day was, as its writer marks it. */
export type Mood = 'great' | 'good' | 'okay' | 'bad' | 'awful';

/** An entry as a change brings it: without its body. */
export interface DiaryEntry {
  id: string;
  diaryId: string;
  date: CalendarDate;
  mood: Mood | null;
  /** Empty for none. */
  weather: string;
  /** Empty for none. */
  place: string;
  createAt: number;
  updateAt: number;
}

/** An entry read on its own or with its page: with its body. */
export interface DiaryEntryDetail extends DiaryEntry {
  body: DocNode | null;
  createdBy: string;
  modifiedBy: string;
}

/** What an entry is changed to; what is left out stays. */
export interface EntryPatch {
  /** A whole new body, sent with `readAt`. */
  body?: DocNode | null;
  /** The `updateAt` the entry was read at: one changed since is refused (409). */
  readAt?: number;
}

export interface Diaries {
  /** The diaries of the reader's own space, oldest first; none of the trash. 404 for any other workspace. */
  diaries(workspaceId: string): Promise<Container[]>;
  /**
   * Makes a diary in the reader's own space, filed under their default
   * category; past the space's limit it is 400 (`too_many_diaries`).
   * `ifNone`: only where the space has none, else its oldest is answered.
   */
  createDiary(workspaceId: string, draft: { title?: string; ifNone?: boolean }): Promise<Container>;
  /** The entries of the diary on `monthDay` of every year, with their bodies, in no promised order. */
  entriesOn(diaryId: string, monthDay: MonthDay): Promise<DiaryEntryDetail[]>;
  /**
   * Makes the entry of a day; where the day has one, that one is answered and
   * nothing is made. A day that is no day, or more than two days past the
   * server's, is 400 at `date`.
   */
  createEntry(diaryId: string, date: CalendarDate): Promise<DiaryEntryDetail>;
  entry(diaryId: string, entryId: string): Promise<DiaryEntryDetail>;
  patchEntry(diaryId: string, entryId: string, patch: EntryPatch): Promise<DiaryEntryDetail>;
  /** Moves the entry to the diary's trash: its day takes a new entry. */
  deleteEntry(diaryId: string, entryId: string): Promise<void>;
}

/** Where a backend keeps its diaries. One by a public link has none. */
export interface DiariesOfBackend {
  readonly diary?: Diaries;
}
