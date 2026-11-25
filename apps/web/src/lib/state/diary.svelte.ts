/**
 * One diary as its page shows it: the entries of one day of the year in every
 * year (`show`), each day's entry opened through an `EntryStore`.
 *
 * The page asks the server for that day alone (`entriesOn`), never for the
 * whole diary. What changes meanwhile is heard on the diary (`watchContainer`):
 * an entry made or changed on the day shown is read again (a change carries no
 * body), also one open here, whose room writes its body now and then: the
 * folded view of it shows what was written; a deleted one goes. The diary's
 * calendar (`calendar`) hears of every entry, of any day.
 */
import type { CalendarDate } from '../logic/dates.ts';
import { monthDayOf, type MonthDay } from '../logic/diary-days.ts';
import type { Container } from '../logic/wire.ts';
import { isBackendError, type Change } from './backend.ts';
import { DiaryCalendar } from './diary-calendar.svelte.ts';
import type { DiaryEntry, DiaryEntryDetail } from './diary-types.ts';
import { EntryStore, type EntryHost } from './entry.svelte.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';

export class DiaryStore implements EntryHost {
  readonly workspace: Workspace;
  readonly diaryId: string;

  status = $state<LoadStatus>('idle');
  /** The diary is not there for the reader (deleted, or never theirs). */
  missing = $state(false);
  /** The day of the year shown. */
  monthDay = $state.raw<MonthDay | undefined>(undefined);
  /** The entries of that day, in every year written. */
  entries = $state.raw<readonly DiaryEntryDetail[]>([]);
  /** Its written days by month, and the runs of them. */
  readonly calendar: DiaryCalendar;

  /** The entry of each day of the page, as it is opened. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown: each store is
  readonly #days = new Map<CalendarDate, EntryStore>();
  #stop: (() => void) | undefined;
  #readings = 0;

  constructor(workspace: Workspace, diaryId: string) {
    this.workspace = workspace;
    this.diaryId = diaryId;
    this.calendar = new DiaryCalendar(this);
  }

  /** The diary as the workspace lists it; undefined while it does not. */
  get diary(): Container | undefined {
    return this.workspace.containers.find((container) => container.id === this.diaryId);
  }

  /** Shows a day of the year: its entries are read, those of the day before let go. */
  async show(monthDay: MonthDay): Promise<void> {
    if (!this.#stop) {
      this.#stop = this.workspace.backend.watchContainer(this.diaryId, (change) => {
        this.#hear(change);
      });
      void this.calendar.count();
    }
    if (this.monthDay !== monthDay) {
      for (const day of this.#days.values()) day.close();
      this.#days.clear();
      this.entries = [];
      this.monthDay = monthDay;
      this.status = 'loading';
    }
    await this.#read(monthDay);
  }

  async #read(monthDay: MonthDay): Promise<void> {
    const diaries = this.workspace.backend.diary;
    if (!diaries) return;
    this.#readings += 1;
    const reading = this.#readings;
    const asked = this.#deletions;
    try {
      const read = await diaries.entriesOn(this.diaryId, monthDay);
      if (reading !== this.#readings || this.monthDay !== monthDay) return;
      // Entries deleted after this read was asked are gone, not resurrected by a late answer.
      const entries = read.filter((entry) => (this.#goneAt.get(entry.id) ?? 0) <= asked);
      this.entries = entries;
      for (const entry of entries) this.#days.get(entry.date)?.adopt(entry);
      // A day whose entry the read has not (deleted unheard, or made after the read was asked).
      for (const day of this.#days.values()) {
        const held = day.entry;
        if (held && !entries.some((one) => one.id === held.id)) this.#gone(held);
      }
      [this.status, this.missing] = ['ready', false];
    } catch (error) {
      if (reading !== this.#readings) return;
      if (isBackendError(error) && (error.status === 404 || error.status === 403)) {
        [this.status, this.missing] = ['failed', true];
      } else {
        this.status = 'failed';
        this.workspace.report('load', error);
      }
    }
  }

  /** The entry of a day of the page, the same one each time it is asked for. */
  day(date: CalendarDate): EntryStore {
    let day = this.#days.get(date);
    if (!day) {
      day = new EntryStore(
        this,
        date,
        this.entries.find((entry) => entry.date === date),
      );
      this.#days.set(date, day);
    }
    return day;
  }

  received(entry: DiaryEntryDetail): void {
    this.calendar.heard([entry]);
    if (monthDayOf(entry.date) !== this.monthDay) return;
    this.entries = [...this.entries.filter((one) => one.id !== entry.id), entry];
  }

  /** Deleted here or elsewhere; the calendar hears of it as every device does (`#hear`). */
  removed(entryId: string): void {
    this.#deletions += 1;
    this.#goneAt.set(entryId, this.#deletions);
    this.entries = this.entries.filter((one) => one.id !== entryId);
  }

  /** How many deletions were heard, and when each entry's was: a read asked before it is dropped. */
  #deletions = 0;
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown
  readonly #goneAt = new Map<string, number>();

  #hear(change: Change): void {
    switch (change.kind) {
      case 'entries':
        this.calendar.heard(change.entries);
        for (const entry of change.entries) this.#changed(entry);
        return;
      case 'blocksDeleted':
        this.calendar.heardDeleted(change.blockIds);
        for (const id of change.blockIds) this.#deleted(id);
        return;
      case 'containerDeleted':
        if (change.containerId === this.diaryId) [this.status, this.missing] = ['failed', true];
        return;
      case 'resync':
        this.calendar.resync();
        if (this.monthDay) void this.#read(this.monthDay);
        return;
      default:
    }
  }

  /** An entry made or changed: read again with its body. A read answered after a later one is dropped. */
  #changed(entry: DiaryEntry): void {
    if (monthDayOf(entry.date) !== this.monthDay) return;
    const shown = this.entries.find((one) => one.id === entry.id);
    if (shown && shown.updatedAt >= entry.updatedAt) return;
    const asked = this.#deletions;
    void this.workspace.backend.diary?.entry(this.diaryId, entry.id).then(
      (detail) => {
        const now = this.entries.find((one) => one.id === detail.id);
        if (now && now.updatedAt > detail.updatedAt) return;
        // Deleted after it was asked for: what it read is gone.
        if ((this.#goneAt.get(detail.id) ?? 0) > asked) return;
        this.received(detail);
        this.#days.get(detail.date)?.adopt(detail);
      },
      () => undefined,
    );
  }

  #deleted(entryId: string): void {
    this.removed(entryId);
    // Its day stays open, with what is being written there: that makes another.
    for (const day of this.#days.values()) if (day.entry?.id === entryId) day.lose();
  }

  /** An entry a read of its day had not: asked for, it is deleted where it is not there. */
  #gone(entry: DiaryEntryDetail): void {
    const asked = this.#deletions;
    void this.workspace.backend.diary?.entry(this.diaryId, entry.id).then(
      (still) => {
        // Deleted after it was asked for: what it read is gone.
        if ((this.#goneAt.get(still.id) ?? 0) > asked) return;
        const now = this.entries.find((one) => one.id === still.id);
        if (now && now.updatedAt > still.updatedAt) return;
        // Made after the read was asked: it is what the day has.
        this.received(still);
        this.#days.get(still.date)?.adopt(still);
      },
      (error: unknown) => {
        if (isBackendError(error) && error.status === 404) {
          this.calendar.heardDeleted([entry.id]);
          this.#deleted(entry.id);
        }
      },
    );
  }

  /** The page goes for good, or is kept to come back to (`persisted`): see `EntryStore.leave`. */
  leave(persisted: boolean): void {
    for (const day of this.#days.values()) day.leave(persisted);
  }

  close(): void {
    this.#stop?.();
    this.#stop = undefined;
    for (const day of this.#days.values()) day.close();
    this.#days.clear();
  }
}

/**
 * Opens the diary of the reader's own space, making the first one where it
 * has none (`ifNone`: two at once make one), in the sidebar at once;
 * undefined when it was refused (told) or the backend has no diaries.
 */
export async function firstDiary(workspace: Workspace): Promise<Container | undefined> {
  const diaries = workspace.backend.diary;
  const shown = workspace.info;
  if (!diaries || !shown) return undefined;
  try {
    const diary = await diaries.createDiary(shown.id, { ifNone: true });
    workspace.receiveContainer(diary);
    return diary;
  } catch (error) {
    workspace.report('save', error);
    return undefined;
  }
}
