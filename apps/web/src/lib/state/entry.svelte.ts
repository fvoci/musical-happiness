/**
 * The entry of one day of a diary as its page has it open: its body, edited
 * together in its room as a card's is (`card.svelte.ts`, `page.svelte.ts`).
 *
 * A day nobody wrote on has no entry, and none is made for it before the
 * reader writes: the first thing they write (`write`) is kept here as a draft
 * while the entry is made, then put into the room's document once that has
 * arrived (`takeDraft`: the editor of the room appends it, after what another
 * device wrote there meanwhile). A draft the room never took (the room did
 * not open, the page went first) is written as the entry's body instead
 * (`patchEntry`), with the version it was made at.
 *
 * An entry the reader emptied here, and leaves empty with nobody else in its
 * room, is deleted as it is left (`closeBody`): an empty day is a day not
 * written, in the diary's calendar too (`isBlank` says what empty is).
 */
import type { DocNode } from '../editor/schema.ts';
import type { CalendarDate } from '../logic/dates.ts';
import { isBackendError, type BodyDoc, type BodyDocState } from './backend.ts';
import { BodyUploads } from './body-uploads.svelte.ts';
import type { DiaryEntryDetail } from './diary-types.ts';
import type { Workspace } from './workspace.svelte.ts';

/** What an entry needs of its diary's store. */
export interface EntryHost {
  readonly workspace: Workspace;
  readonly diaryId: string;
  /** An entry made, read or deleted here: the page's list follows. */
  received(entry: DiaryEntryDetail): void;
  removed(entryId: string): void;
}

/**
 * Whether an entry holds nothing the reader put in it: no text, no image or
 * file in its body (`blankBody`, as its editor says), and none of the day's
 * notes. What a day may carry besides its body is counted here.
 */
export function isBlank(entry: DiaryEntryDetail, blankBody: boolean): boolean {
  return blankBody && entry.mood === null && entry.weather === '' && entry.place === '';
}

export class EntryStore {
  readonly host: EntryHost;
  readonly date: CalendarDate;

  /** The entry, once there is one. */
  entry = $state.raw<DiaryEntryDetail | undefined>(undefined);
  /** The body in its room, while it is open there (`openBody`). */
  room = $state.raw<BodyDocState | undefined>(undefined);
  /** Why things of the reader's were dropped, each once, until dismissed. */
  lost = $state.raw<readonly string[]>([]);
  /** What the reader wrote before the room took it; `null` for nothing. */
  draft = $state.raw<DocNode | null>(null);
  /** The reader writes in the draft (emptied again, too) and the room has not taken it yet. */
  drafting = $state(false);
  /** The draft could not be kept: the entry was not made, or its body not written. */
  unsaved = $state(false);

  #bodyDoc: BodyDoc | undefined;
  /** The body is asked to be open (`openBody`): it opens once there is an entry. */
  #wanted = false;
  #making: Promise<void> | undefined;
  /** The reader changed the room's document while it was open here. */
  #changedHere = false;
  /** The editor of the room's document holds nothing (`noteBlank`). */
  #blankBody = false;
  #closed = false;

  constructor(host: EntryHost, date: CalendarDate, entry?: DiaryEntryDetail) {
    this.host = host;
    this.date = date;
    this.entry = entry;
  }

  /** The body as it was read, shown until the room's document has arrived. */
  get body(): DocNode | null {
    return this.entry?.body ?? null;
  }

  /** The entry of the day was made or changed elsewhere (another device): this one is it. */
  adopt(entry: DiaryEntryDetail): void {
    if (entry.date !== this.date || this.#closed) return;
    const first = this.entry === undefined;
    this.entry = entry;
    if (first && this.#wanted) this.openBody();
  }

  /**
   * The reader wrote in a day without an entry: what they wrote is the draft,
   * and the entry is made the first time it is not empty.
   */
  write(draft: DocNode | null): void {
    this.draft = draft;
    this.drafting = true;
    if (draft !== null && !this.entry && !this.#making) {
      this.#making = this.#make().finally(() => {
        this.#making = undefined;
      });
    }
  }

  async #make(): Promise<void> {
    const diaries = this.host.workspace.backend.diary;
    if (!diaries) return;
    try {
      const entry = await diaries.createEntry(this.host.diaryId, this.date);
      this.host.received(entry);
      this.adopt(entry);
      // Left before the entry was there: what was written goes as its body.
      if (!this.#wanted || this.#closed) await this.#flush();
    } catch (error) {
      this.unsaved = true;
      this.host.workspace.report('save', error);
    }
  }

  /** The draft, for the editor of the room's document to put in; given once. */
  takeDraft(): DocNode | null {
    const draft = this.draft;
    [this.draft, this.drafting] = [null, false];
    return draft;
  }

  /** What the editor of the room's document holds now: whether it is empty. */
  noteBlank(blank: boolean): void {
    this.#blankBody = blank;
  }

  // --------------------------------------------------------------- body

  /** Opens the body in its room, once there is an entry; unless it is open, or the backend has no rooms. */
  openBody(): void {
    this.#wanted = true;
    const backend = this.host.workspace.backend;
    const workspaceId = this.host.workspace.info?.id;
    const entryId = this.entry?.id;
    if (this.#bodyDoc || this.#closed || !entryId || !backend.openDoc || !workspaceId) return;
    const opened = backend.openDoc(workspaceId, this.host.diaryId, entryId, {
      state: (state) => {
        if (this.#bodyDoc !== opened) return;
        if (state.pending) this.#changedHere = true;
        this.room = state;
      },
      discarded: (why) => {
        if (this.#bodyDoc === opened) this.sayLost(why);
      },
    });
    this.#bodyDoc = opened;
    this.room = opened.state;
  }

  /**
   * Leaves the body's room, what waits sent first. A draft it never took is
   * written as the body; an entry the reader emptied here is deleted.
   */
  closeBody(): void {
    this.#wanted = false;
    const opened = this.#bodyDoc;
    const room = this.room;
    this.#bodyDoc = undefined;
    this.room = undefined;
    if (this.draft !== null && this.entry) void this.#flush();
    const entry = this.entry;
    const alone = (room?.carets?.getStates().size ?? 0) <= 1;
    const emptied =
      entry && room?.doc && this.#changedHere && alone && isBlank(entry, this.#blankBody);
    this.#changedHere = false;
    if (!opened) return;
    if (!emptied) {
      opened.close();
      return;
    }
    void opened.settle().then(() => {
      opened.close();
      return this.#delete(entry);
    });
  }

  /** The page goes: nothing more is opened here. */
  close(): void {
    this.closeBody();
    this.#closed = true;
    this.#bodyUploads.giveUp();
  }

  async #flush(): Promise<void> {
    const entry = this.entry;
    const diaries = this.host.workspace.backend.diary;
    const draft = this.takeDraft();
    if (!entry || !diaries || draft === null) return;
    try {
      this.host.received(
        await diaries.patchEntry(this.host.diaryId, entry.id, {
          body: draft,
          readAt: entry.updateAt,
        }),
      );
    } catch (error) {
      this.unsaved = true;
      this.host.workspace.report('save', error);
    }
  }

  async #delete(entry: DiaryEntryDetail): Promise<void> {
    try {
      await this.host.workspace.backend.diary?.deleteEntry(this.host.diaryId, entry.id);
      if (this.entry?.id === entry.id) this.entry = undefined;
      this.host.removed(entry.id);
    } catch (error) {
      // Gone already (deleted elsewhere) is what was wanted.
      if (!isBackendError(error) || error.status !== 404) this.host.workspace.report('save', error);
    }
  }

  retryBody(): void {
    this.#bodyDoc?.retry();
  }

  sayLost(why: string): void {
    if (!this.lost.includes(why)) this.lost = [...this.lost, why];
  }

  dismissLost(): void {
    this.lost = [];
  }

  /** Uploads a file for the body, a file of the diary; closing the page gives it up. */
  uploadFile(file: File): Promise<string> {
    return this.#bodyUploads.run(file, (options) =>
      this.host.workspace.backend.upload(this.host.diaryId, file, options),
    );
  }

  readonly #bodyUploads = new BodyUploads(() => this.host.workspace);
}
