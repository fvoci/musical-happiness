/**
 * One editor of a file at a time: the file's lease, which the server keeps
 * (`holdFileLease`). "Edit" takes it; the editor holds on to it every 30
 * seconds while it is open and as soon as its window is shown again, and lets
 * it go when it is closed, or with the page as the page goes (a request that
 * outlives it). A window that dies keeps it only until its term ends (90
 * seconds, the server's): nothing is held for ever.
 *
 * A lease keeps nobody from writing a version: a save is held to the version
 * it was made from. A lease lost (a window asleep past its term while
 * somebody else took the file's) is told to the writer, and the editor goes
 * on.
 *
 * The viewer asks whether somebody edits the file (`LeaseWatch`) as it opens,
 * every 15 seconds while it is shown, and after a lease was refused.
 */

/** Whether somebody has a file open in the editor, and who. */
export interface FileLease {
  editing: boolean;
  /** Who, where the reader may know (the reader included); empty for nobody, and for somebody else. */
  editorId: string;
}

/** A lease the reader holds. */
export interface HeldFileLease {
  leaseId: string;
  /** How many seconds it lasts unless held on to before then. */
  seconds: number;
}

/** What a file's lease is reached through: the file's versions (versions.svelte.ts). */
export interface LeaseSource {
  /** Who the reader is, where known. */
  readonly me: string | undefined;
  lease(): Promise<FileLease>;
  /** Takes the lease (`leaseId` empty) or holds on to it; `'editing'` while another holds the file's. */
  holdLease(leaseId: string): Promise<HeldFileLease | 'editing'>;
  /** Lets it go; `keepalive` as the page goes. */
  releaseLease(leaseId: string, keepalive: boolean): Promise<void>;
  /** Reads the names of those it names. */
  learnUsers(userIds: string[]): Promise<void>;
}

export interface LeaseTimes {
  /** How often the editor holds on to its lease, in milliseconds. */
  holdEvery: number;
  /** How often the viewer asks who edits the file. */
  watchEvery: number;
}

export const LEASE_TIMES: LeaseTimes = { holdEvery: 30_000, watchEvery: 15_000 };

/** What taking a lease came to: it is held, another holds the file's, or it could not be asked. */
export type Taking = 'held' | 'editing' | 'failed';

/** The lease of a file open in the editor. */
export class EditLease {
  /** Somebody else holds the file's lease now: this one ended unheld, and was taken meanwhile. */
  lost = $state(false);

  readonly #source: LeaseSource;
  readonly #times: LeaseTimes;
  #leaseId = '';
  #timer: ReturnType<typeof setInterval> | undefined;
  #ended = false;

  constructor(source: LeaseSource, times: LeaseTimes = LEASE_TIMES) {
    this.#source = source;
    this.#times = times;
  }

  /** Takes the lease, and holds on to it from then until it is let go. */
  async take(): Promise<Taking> {
    let held: HeldFileLease | 'editing';
    try {
      held = await this.#source.holdLease('');
    } catch {
      return 'failed';
    }
    if (held === 'editing') return 'editing';
    this.#leaseId = held.leaseId;
    if (this.#ended) {
      // Let go of before it was given.
      void this.#letGo(false);
      return 'failed';
    }
    this.#timer = setInterval(() => {
      void this.#holdOn();
    }, this.#times.holdEvery);
    document.addEventListener('visibilitychange', this.#onVisibility);
    window.addEventListener('pagehide', this.#onHide);
    window.addEventListener('pageshow', this.#onShow);
    return 'held';
  }

  async #holdOn(): Promise<void> {
    if (this.#ended) return;
    let held: HeldFileLease | 'editing';
    try {
      held = await this.#source.holdLease(this.#leaseId);
    } catch {
      // Not reached: asked again at the next turn, within the lease's term.
      return;
    }
    if (held === 'editing') {
      this.lost = true;
      return;
    }
    // A lease that ended and that nobody took is taken anew, under another name.
    this.#leaseId = held.leaseId;
    this.lost = false;
    // Let go of while it was asked for: what was given since goes too.
    if (this.#over()) void this.#letGo(false);
  }

  /** Whether the lease was let go of, read anew after a wait. */
  #over(): boolean {
    return this.#ended;
  }

  readonly #onVisibility = (): void => {
    if (document.visibilityState === 'visible') void this.#holdOn();
  };

  /** The page goes (closed, or left): the lease goes with it. */
  readonly #onHide = (): void => {
    void this.#letGo(true);
  };

  /** The page came back from the browser's cache of pages left: the lease is taken anew. */
  readonly #onShow = (event: PageTransitionEvent): void => {
    if (event.persisted) void this.#holdOn();
  };

  /** Lets the lease held go; settles when the server answered, or did not. */
  #letGo(keepalive: boolean): Promise<void> {
    const leaseId = this.#leaseId;
    if (!leaseId) return Promise.resolve();
    this.#leaseId = '';
    return this.#source.releaseLease(leaseId, keepalive).catch(() => undefined);
  }

  /**
   * Lets the lease go, and stops holding on to it: the editor is closed.
   * Settles when the server answered, or did not.
   */
  release(): Promise<void> {
    if (this.#ended) return Promise.resolve();
    this.#ended = true;
    clearInterval(this.#timer);
    document.removeEventListener('visibilitychange', this.#onVisibility);
    window.removeEventListener('pagehide', this.#onHide);
    window.removeEventListener('pageshow', this.#onShow);
    return this.#letGo(false);
  }
}

/**
 * What "Edit" came to: the lease is held and the version on screen is the
 * newest (`held`), another edits the file, the lease could not be asked for,
 * or a newer version was written since the one on screen was read.
 */
export type Start = { as: 'held'; lease: EditLease } | { as: 'editing' | 'failed' | 'newer' };

/**
 * Takes a file's lease to edit its version `version`, then asks which version
 * is the newest (`newest`, undefined where unknown): a version written since
 * the one on screen was read is not edited over, and the lease goes again.
 */
export async function startEditing(
  source: LeaseSource,
  version: number,
  newest: () => Promise<number | undefined>,
): Promise<Start> {
  const lease = new EditLease(source);
  const taken = await lease.take();
  if (taken !== 'held') return { as: taken };
  const current = await newest().catch(() => undefined);
  if (current === undefined || current === version) return { as: 'held', lease };
  void lease.release();
  return { as: 'newer' };
}

/** Who edits a file, as the viewer asks it. */
export class LeaseWatch {
  /** As last read; undefined until it is, and when it could not be. */
  lease = $state.raw<FileLease>();

  readonly #source: LeaseSource;
  readonly #every: number;

  constructor(source: LeaseSource, every = LEASE_TIMES.watchEvery) {
    this.#source = source;
    this.#every = every;
  }

  /** Somebody edits the file in another window or tab: the reader in another, or somebody else. */
  get editing(): boolean {
    return this.lease?.editing === true;
  }

  /** Who: the reader (in another window), somebody named, or somebody else. */
  get editor(): { who: 'me' } | { who: 'named'; userId: string } | { who: 'someone' } | undefined {
    const lease = this.lease;
    if (!lease?.editing) return undefined;
    if (lease.editorId === '') return { who: 'someone' };
    if (lease.editorId === this.#source.me) return { who: 'me' };
    return { who: 'named', userId: lease.editorId };
  }

  /** Asks again, and learns the name of who is named. */
  async read(): Promise<void> {
    try {
      const lease = await this.#source.lease();
      const { editorId } = lease;
      if (editorId !== '' && editorId !== this.#source.me) {
        await this.#source.learnUsers([editorId]).catch(() => undefined);
      }
      this.lease = lease;
    } catch {
      this.lease = undefined;
    }
  }

  /**
   * Asks once `first` settled (a lease of the reader's being let go), and every while
   * after while the page is shown, until the returned function is called.
   */
  watch(first: Promise<unknown> = Promise.resolve()): () => void {
    void first.finally(() => this.read());
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void this.read();
    }, this.#every);
    return () => {
      clearInterval(timer);
    };
  }
}
