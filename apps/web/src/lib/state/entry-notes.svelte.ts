/**
 * The notes of a day (its mood, weather and place) as the reader changes them
 * on its page (`EntryStore.mark`).
 *
 * A change is the day's at once, before it is sent: it is what the page shows
 * and what says whether the day is blank (`isBlank`). It is sent on a patch of
 * its own, without a version: a mood at once, what is typed once the typing
 * pauses or the field is left. What one device sent last is what the entry
 * keeps, whichever device sent it. A change not answered yet stays over what
 * is heard meanwhile (`over`), so another device's change does not take back
 * what is being typed here; one refused waits to be sent with the next.
 */
import { isBackendError } from './backend.ts';
import type { DayNotes, Diaries, DiaryEntryDetail } from './diary-types.ts';

/** How long after the last key a weather or a place is sent. */
export const TYPING_PAUSE = 800;

/** What the notes need of the entry they are of. */
export interface NotesHost {
  readonly diaries: Diaries | undefined;
  readonly diaryId: string;
  /** The entry they are sent to, once there is one. */
  readonly entryId: string | undefined;
  /** A patch was answered with the entry it made. */
  written(entry: DiaryEntryDetail): void;
  /** A patch was refused; what it held waits to be sent again. */
  refused(error: unknown): void;
}

const NONE: Partial<DayNotes> = {};

export class EntryNotes {
  readonly #host: NotesHost;
  /** Changed here and not sent yet. */
  #waiting = $state.raw<Partial<DayNotes>>(NONE);
  /** On its way. */
  #sending = $state.raw<Partial<DayNotes>>(NONE);
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(host: NotesHost) {
    this.#host = host;
  }

  /** Something changed here is not answered yet. */
  get pending(): boolean {
    return this.#waiting !== NONE || this.#sending !== NONE;
  }

  /** Notes as they were read or heard, with what is not answered yet over them. */
  over<T extends DayNotes>(notes: T): T {
    return this.pending ? { ...notes, ...this.#sending, ...this.#waiting } : notes;
  }

  /** The reader changed notes: sent now, or once the typing pauses (`later`). */
  change(change: Partial<DayNotes>, when: 'now' | 'later'): void {
    this.#waiting = { ...this.#waiting, ...change };
    clearTimeout(this.#timer);
    if (when === 'now') void this.send();
    else
      this.#timer = setTimeout(() => {
        void this.send();
      }, TYPING_PAUSE);
  }

  /**
   * Sends what waits, once there is an entry and nothing else is on its way;
   * what is changed meanwhile goes after it. `keepalive`: as the page goes.
   */
  async send(options: { keepalive?: boolean } = {}): Promise<void> {
    clearTimeout(this.#timer);
    const { diaries, diaryId, entryId } = this.#host;
    const sending = this.#waiting;
    if (!diaries || !entryId || this.#sending !== NONE || sending === NONE) return;
    [this.#sending, this.#waiting] = [sending, NONE];
    let written: DiaryEntryDetail;
    try {
      written = await diaries.patchEntry(diaryId, entryId, sending, options);
    } catch (error) {
      const later = this.#waiting;
      [this.#sending, this.#waiting] = [NONE, { ...sending, ...later }];
      // An entry deleted elsewhere takes the notes to the one made next (`EntryStore.lose`).
      if (!isBackendError(error) || error.status !== 404) this.#host.refused(error);
      return;
    }
    this.#sending = NONE;
    this.#host.written(written);
    if (this.#waiting !== NONE && !options.keepalive) await this.send();
  }

  /** Nothing is sent by itself any more: the page goes. */
  stop(): void {
    clearTimeout(this.#timer);
  }
}
