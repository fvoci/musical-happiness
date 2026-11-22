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
 * (`patchEntry`), after the body the entry has (`appended`), with the version
 * it was read at; the draft is this store's until that write is answered
 * (`flushing`: nobody writes in it, and the room's editor waits), and is
 * kept, to be written again, where it was refused.
 *
 * An entry the reader emptied here, or made here and left blank, and leaves
 * blank with nobody else in its room, is deleted as it is left (`closeBody`,
 * and `leave` as the page goes): an empty day is a day not written, in the
 * diary's calendar too (`isBlank` says what empty is).
 *
 * The day's mood, weather and place are marked here too (`mark`, `notes`):
 * they are the day's at once, sent apart from the body (`EntryNotes`), and a
 * day without an entry has one made for them as for its first words.
 */
import type { DocNode } from '../editor/schema.ts';
import type { CalendarDate } from '../logic/dates.ts';
import { isBackendError, type BodyDoc, type BodyDocState } from './backend.ts';
import { BodyUploads } from './body-uploads.svelte.ts';
import type { DayNotes, Diaries, DiaryEntryDetail } from './diary-types.ts';
import { EntryNotes, type NotesHost } from './entry-notes.svelte.ts';
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
export function isBlank(entry: DayNotes, blankBody: boolean): boolean {
  return blankBody && entry.mood === null && entry.weather === '' && entry.place === '';
}

/** A body with what was drafted after it; the draft alone where the body holds nothing. */
export function appended(body: DocNode | null, draft: DocNode): DocNode {
  const held = (body?.content ?? []).filter(
    (node) => node.type !== 'paragraph' || (node.content?.length ?? 0) > 0,
  );
  return body && held.length > 0
    ? { ...body, content: [...(body.content ?? []), ...(draft.content ?? [])] }
    : draft;
}

/** The notes of a day nobody marked. */
const NO_NOTES: DayNotes = { mood: null, weather: '', place: '' };

/** How long after the entry could not be made it is not asked for again, however much is typed. */
const MAKE_AGAIN_AFTER = 5000;

/** A refusal that may pass by itself: no answer, too many requests, the server's own trouble. */
function passing(error: unknown): boolean {
  return (
    isBackendError(error) && (error.status === 0 || error.status === 429 || error.status >= 500)
  );
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
  /** The draft is being written as the body: it is that write's until answered, and is not written in meanwhile. */
  flushing = $state(false);

  #bodyDoc: BodyDoc | undefined;
  /** The body is asked to be open (`openBody`): it opens once there is an entry. */
  #wanted = false;
  #making: Promise<void> | undefined;
  /** When the entry may be asked for again after it could not be made. */
  #makeAgainAt = 0;
  /** Asks for it then, typed meanwhile or not. */
  #again: ReturnType<typeof setTimeout> | undefined;
  /** The reader changed the room's document while it was open here, or made the entry here. */
  #changedHere = false;
  /** The editor of the room's document holds nothing (`noteBlank`). */
  #blankBody = false;
  #closed = false;
  readonly #notes: EntryNotes;

  constructor(host: EntryHost, date: CalendarDate, entry?: DiaryEntryDetail) {
    this.host = host;
    this.date = date;
    this.entry = entry;
    this.#notes = new EntryNotes(this.#notesHost());
  }

  #notesHost(): NotesHost {
    const host = this.host;
    const entry = (): DiaryEntryDetail | undefined => this.entry;
    const drafted = (): boolean => this.draft !== null;
    return {
      get diaries() {
        return host.workspace.backend.diary;
      },
      diaryId: host.diaryId,
      get entryId() {
        return entry()?.id;
      },
      get held() {
        return drafted();
      },
      written: (written) => {
        const held = this.entry;
        if (held?.id !== written.id) return;
        // An answer older than the entry held (its body written meanwhile) leaves it as it is.
        this.entry = this.#notes.over(held.updateAt > written.updateAt ? held : written);
        host.received(this.entry);
      },
      refused: (error) => {
        host.workspace.report('save', error);
      },
    };
  }

  /** The day's mood, weather and place, as marked here last. */
  get notes(): DayNotes {
    return this.#notes.over(this.entry ?? NO_NOTES);
  }

  /**
   * The reader marked the day's mood, weather or place (`later`: as they type
   * it): the day's at once; a day without an entry has one made for it.
   */
  mark(change: Partial<DayNotes>, when: 'now' | 'later' = 'now'): void {
    this.#notes.change(change, when);
    this.#changedHere = true;
    const entry = this.entry;
    if (!entry) {
      this.#makeIfNone();
      return;
    }
    this.entry = this.#notes.over(entry);
    this.host.received(this.entry);
  }

  /** What was typed in the notes goes now: the field was left. */
  sendNotes(): void {
    void this.#notes.send();
  }

  /** Something wants the day to have an entry: a draft, or notes marked that hold something. */
  #wantsEntry(): boolean {
    return this.draft !== null || !isBlank(this.#notes.over(NO_NOTES), true);
  }

  /** The body as it was read, shown until the room's document has arrived. */
  get body(): DocNode | null {
    return this.entry?.body ?? null;
  }

  /** The entry of the day was made or changed elsewhere (another device): this one is it. */
  adopt(entry: DiaryEntryDetail): void {
    if (entry.date !== this.date || this.#closed) return;
    const held = this.entry;
    // Read before what is held was: it is no news.
    if (held?.id === entry.id && held.updateAt > entry.updateAt) return;
    // Another than the one held (deleted unheard, and the day written again elsewhere): its room is left.
    if (held && held.id !== entry.id) this.lose();
    const first = this.entry === undefined;
    if (first) this.#notes.forgetBlank();
    this.entry = this.#notes.over(entry);
    if (first) void this.#notes.send();
    if (first && this.#wanted) this.openBody();
  }

  /** The entry was deleted elsewhere: its room is left, and what is written next makes another. */
  lose(): void {
    const opened = this.#bodyDoc;
    [this.#bodyDoc, this.room, this.entry, this.#changedHere] = [
      undefined,
      undefined,
      undefined,
      false,
    ];
    this.#blankBody = false;
    opened?.close();
    this.#notes.forgetBlank();
    if (this.#wantsEntry() && !this.#closed) this.#makeIfNone();
  }

  /**
   * The reader wrote in a day without an entry: what they wrote is the draft,
   * and the entry is made the first time it is not empty.
   */
  write(draft: DocNode | null): void {
    this.draft = draft;
    this.drafting = true;
    if (draft !== null) this.#makeIfNone();
    // No draft holds the notes any more.
    else void this.#notes.send();
  }

  /** Makes the day's entry, once at a time, unless it has one or was refused a moment ago. */
  #makeIfNone(): void {
    if (this.entry || this.#making || Date.now() < this.#makeAgainAt) return;
    this.#making = this.#make().finally(() => {
      this.#making = undefined;
    });
  }

  async #make(): Promise<void> {
    const diaries = this.host.workspace.backend.diary;
    if (!diaries) return;
    try {
      const made = await diaries.createEntry(this.host.diaryId, this.date);
      // Made elsewhere first, it is answered as it is: what holds nothing here clears nothing of it.
      this.#notes.forgetBlank();
      const entry = this.#notes.over(made);
      this.host.received(entry);
      this.unsaved = false;
      if (this.entry === undefined) {
        // Made for what was written here: left blank here, it goes (`closeBody`).
        [this.entry, this.#changedHere] = [entry, true];
      }
      // Sent where no draft holds them; a draft carries them (`#flush`).
      const noted = this.#notes.send();
      // Left before the entry was there: what was written goes as its body.
      if (this.#closed || !this.#wanted) {
        await noted;
        await this.#flush();
      } else this.openBody();
    } catch (error) {
      this.#makeAgainAt = Date.now() + MAKE_AGAIN_AFTER;
      // Said once, until the entry is made; asked for again by itself where that may pass.
      if (!this.unsaved) this.host.workspace.report('save', error);
      this.unsaved = true;
      if (!passing(error)) return;
      this.#again = setTimeout(() => {
        [this.#again, this.#makeAgainAt] = [undefined, 0];
        if (!this.#closed && this.#wantsEntry()) this.#makeIfNone();
      }, MAKE_AGAIN_AFTER);
    }
  }

  /** The draft, for the editor of the room's document to put in; given once, and not while it is being written. */
  takeDraft(): DocNode | null {
    if (this.flushing) return null;
    const draft = this.draft;
    [this.draft, this.drafting] = [null, false];
    // The room writes the draft as it takes it: the notes it held go now.
    void this.#notes.send();
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
   * The entry the reader leaves blank, alone in its room, having emptied or
   * made it here; never one with a draft still to be written into it.
   */
  #emptied(): DiaryEntryDetail | undefined {
    const entry = this.entry;
    const room = this.room;
    const alone = (room?.carets?.getStates().size ?? 0) <= 1;
    const held = this.draft !== null || this.flushing;
    const left = this.#changedHere && alone && !held;
    const blank = entry && isBlank(this.#notes.over(entry), this.#blankBody);
    return entry && room?.doc && left && blank ? entry : undefined;
  }

  /**
   * Leaves the body's room, what waits sent first. A draft it never took is
   * written as the body; an entry left blank here is deleted.
   */
  closeBody(): void {
    this.#wanted = false;
    const opened = this.#bodyDoc;
    const emptied = this.#emptied();
    // An entry left blank keeps what waits until it is deleted (`#delete`).
    if (emptied) this.#notes.stop();
    else void this.#notes.send();
    [this.#bodyDoc, this.room, this.#changedHere, this.#blankBody] = [
      undefined,
      undefined,
      false,
      false,
    ];
    if (this.draft !== null && this.entry) void this.#flush();
    if (!opened) return;
    if (!emptied) {
      opened.close();
      return;
    }
    void opened.settle().then(() => {
      opened.close();
      // Opened again meanwhile: it is written in, not left.
      if (this.#bodyDoc || this.#wanted) {
        void this.#notes.send();
        return undefined;
      }
      return this.#delete(emptied);
    });
  }

  /**
   * The page goes (the tab closed, another address loaded, or kept to come
   * back to: `persisted`): what `closeBody` would send is sent at once, to be
   * sent still once the page has gone. A draft goes with the notes not
   * answered yet in one write (`#flush`); else the notes go in one of their
   * own (`EntryNotes.leave`). A page kept to come back to deletes nothing.
   */
  leave(persisted: boolean): void {
    const diaries = this.host.workspace.backend.diary;
    const emptied = this.#emptied();
    if (!diaries || !this.entry) return;
    if (this.draft !== null && !this.flushing) void this.#flush({ keepalive: true });
    else if (emptied && !persisted) {
      void diaries
        .deleteEntry(this.host.diaryId, emptied.id, { keepalive: true })
        .catch(() => undefined);
    } else void this.#notes.leave();
  }

  /** The page goes: nothing more is opened here; a draft or notes whose entry could not be made are tried once more. */
  close(): void {
    this.closeBody();
    this.#closed = true;
    clearTimeout(this.#again);
    this.#notes.stop();
    if (this.#wantsEntry() && !this.entry && !this.#making) {
      this.#makeAgainAt = 0;
      this.#makeIfNone();
    }
    this.#bodyUploads.giveUp();
  }

  /** Writes the draft as the body, with the notes it held (`NotesHost.held`). */
  async #flush(options: { keepalive?: boolean } = {}): Promise<void> {
    const entry = this.entry;
    const diaries = this.host.workspace.backend.diary;
    const draft = this.draft;
    if (!entry || !diaries || draft === null || this.flushing) return;
    this.flushing = true;
    const notes = this.#notes.unanswered() ?? {};
    try {
      const written = await this.#writeDraft(diaries, entry, { draft, notes }, options);
      // Deleted elsewhere meanwhile: what was written went to the trash with
      // it, and the draft stays, for the entry made again (`lose`).
      if (this.entry?.id !== entry.id) return;
      this.#notes.carried(notes);
      if (this.draft === draft) [this.draft, this.drafting] = [null, false];
      // Written here, it is not blank: it is emptied here only by a change in its room.
      [this.entry, this.unsaved, this.#changedHere] = [this.#notes.over(written), false, false];
      this.host.received(this.entry);
      void this.#notes.send();
    } catch (error) {
      this.unsaved = true;
      this.host.workspace.report('save', error);
    } finally {
      this.flushing = false;
    }
  }

  /** The draft after the body the entry has, with `notes`; read again once where it changed meanwhile. */
  async #writeDraft(
    diaries: Diaries,
    entry: DiaryEntryDetail,
    { draft, notes }: { draft: DocNode; notes: Partial<DayNotes> },
    options: { keepalive?: boolean },
  ): Promise<DiaryEntryDetail> {
    const write = (read: DiaryEntryDetail): Promise<DiaryEntryDetail> =>
      diaries.patchEntry(
        this.host.diaryId,
        read.id,
        { ...notes, body: appended(read.body, draft), readAt: read.updateAt },
        options,
      );
    try {
      return await write(entry);
    } catch (error) {
      if (!isBackendError(error) || error.status !== 409) throw error;
      return write(await diaries.entry(this.host.diaryId, entry.id));
    }
  }

  async #delete(entry: DiaryEntryDetail): Promise<void> {
    try {
      await this.host.workspace.backend.diary?.deleteEntry(this.host.diaryId, entry.id);
      // Its room, opened again as it was deleted, is left; what was marked since stays.
      if (this.entry?.id === entry.id) this.lose();
      this.host.removed(entry.id);
    } catch (error) {
      // Gone already (deleted elsewhere) is what was wanted.
      if (isBackendError(error) && error.status === 404) return;
      this.host.workspace.report('save', error);
      // Kept, it keeps what was emptied in it.
      void this.#notes.send();
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
