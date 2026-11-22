/**
 * The versions of an open card's body, as their dialog reads them: the
 * list, read when this is made (the dialog makes one each time it opens),
 * the version chosen with its body, and going back to it.
 *
 * Reading the list and restoring send what the reader typed to the card's
 * room first: the list then says of no version that it is the card's body
 * while the reader's own typing is not in it, and the server keeps what was
 * typed with the body before as a version. What could not be sent in time
 * (no connection) is dropped with the document the restore replaces, and
 * said lost as for any body written whole.
 *
 * Keeping a version now (`keepVersion`, from the dialog's button and from
 * the keys in the body) sends what was typed first too: what is kept has it.
 *
 * The version chosen is shown as what it changed (`compare`,
 * version-compare.svelte.ts): from the version before it, or from the body
 * now, which a source gives with what was typed in the room
 * (`VersionsSource.current`).
 *
 * A wiki's page has versions of its body as a card has: read the same way
 * (`pageVersions`), with the wiki in place of the board.
 */
import type { Doc as YDoc } from 'yjs';

import type { DocNode } from '$lib/editor/schema.ts';
import {
  isBackendError,
  type BoardStore,
  type BodyVersion,
  type BodyVersionContent,
  type BodyVersions,
  type CardStore,
  type KeptBodyVersion,
  type PageStore,
} from '$lib/state/index.ts';

import { VersionComparing } from './version-compare.svelte.ts';

/** Whose versions a reading reads: one card's, or one page's. */
export interface VersionsSource {
  readonly workspace: BoardStore['workspace'];
  readonly versions: BodyVersions;
  /** The board, or the wiki. */
  readonly ownerId: string;
  /** The card, or the page. */
  readonly itemId: string;
  /** Sends what the reader typed to the room (`CardStore.settleBody`). */
  settle(): Promise<void>;
  /**
   * Whether what the reader types goes to the room now (`BodyDocState.writable`):
   * otherwise a version kept would not be the body they see.
   */
  writable(): boolean;
  /** Whether the body is still being opened (read, or its room joined): it may be written soon. */
  opening(): boolean;
  /**
   * The body now: what its room holds, with what the reader and the others
   * typed, or the stored body when there is no room.
   */
  current(): Promise<DocNode | null>;
}

/** The body a room's document holds, or the stored one when there is no room or it holds none. */
/** Whether a body is still being opened: read, or its room joined or sent again. */
function opening(item: Pick<CardStore, 'status' | 'room'>): boolean {
  const status = item.room?.status;
  if (status === undefined) return item.status !== 'failed';
  return status === 'joining' || status === 'receiving';
}

async function bodyNow(
  room: { doc: YDoc | undefined } | undefined,
  stored: DocNode | null,
): Promise<DocNode | null> {
  if (!room?.doc) return stored;
  const { bodyOfDoc } = await import('$lib/editor/room-body.ts');
  const body = bodyOfDoc(room.doc);
  return body === undefined ? stored : body;
}

/** The versions of a card's body. */
export function cardVersions(board: BoardStore, card: CardStore): VersionsSource {
  return {
    workspace: board.workspace,
    versions: board.workspace.backend.bodyVersions,
    ownerId: board.boardId,
    itemId: card.cardId,
    settle: () => card.settleBody(),
    writable: () => card.room?.writable ?? false,
    opening: () => opening(card),
    current: () => bodyNow(card.room, card.body),
  };
}

/** The versions of a page's body, for a backend that has wikis. */
export function pageVersions(page: PageStore): VersionsSource {
  const wikis = page.wiki.workspace.backend.wiki;
  if (!wikis) throw new Error('this backend has no wikis');
  return {
    workspace: page.wiki.workspace,
    versions: wikis.bodyVersions,
    ownerId: page.wiki.wikiId,
    itemId: page.pageId,
    settle: () => page.settleBody(),
    writable: () => page.room?.writable ?? false,
    opening: () => opening(page),
    current: () => bodyNow(page.room, page.body),
  };
}

/**
 * What came of asking to keep a version now: the server's answer, that there
 * was none (`failed`), or that none was asked for, the body being one that
 * cannot be written for now (`notNow`) or not yet, being opened (`opening`).
 */
export type KeepOutcome = KeptBodyVersion['outcome'] | 'failed' | 'notNow' | 'opening';

/** Keeps the body as a version now, once what the reader typed is in the room. */
export async function keepVersion(source: VersionsSource): Promise<KeepOutcome> {
  // What was typed may not reach the room: what would be kept is not what is shown.
  if (!source.writable()) return source.opening() ? 'opening' : 'notNow';
  try {
    await source.settle();
    // The room was lost meanwhile: what was typed last may not be in it.
    if (!source.writable()) return 'notNow';
    return (await source.versions.keep(source.ownerId, source.itemId)).outcome;
  } catch {
    return 'failed';
  }
}

/**
 * Why a version was not restored: the card has it no more (`gone`), it
 * cannot be stored any more or names a file (of an image or a file block)
 * that is gone (`unrestorable`), or the write did not go through.
 */
export type RestoreRefusal = 'gone' | 'unrestorable' | 'failed';

export class VersionsReading {
  status = $state<'loading' | 'ready' | 'failed'>('loading');
  /** The newest first. */
  versions = $state.raw<BodyVersion[]>([]);
  /** The version being read, as the list has it. */
  chosen = $state.raw<BodyVersion>();
  /** Its body, once it is here. */
  content = $state.raw<BodyVersionContent>();
  /** `gone`: the card has the version no more (the newest are kept). */
  contentStatus = $state<'loading' | 'ready' | 'failed' | 'gone'>('loading');
  restoring = $state(false);
  refusal = $state<RestoreRefusal>();
  keeping = $state(false);
  /** What came of the last ask to keep a version here. */
  kept = $state<KeepOutcome>();

  /** What the version being read is compared with. */
  readonly compare: VersionComparing;

  readonly #source: VersionsSource;

  constructor(source: VersionsSource) {
    this.#source = source;
    this.compare = new VersionComparing(this, {
      read: async (version) =>
        (await source.versions.read(source.ownerId, source.itemId, version)).body,
      current: () => source.current(),
    });
    void this.read();
  }

  get #versions(): BodyVersions {
    return this.#source.versions;
  }

  /** Reads the list (again). */
  async read(): Promise<void> {
    this.status = 'loading';
    try {
      this.#take(await this.#list());
      this.status = 'ready';
    } catch {
      this.status = 'failed';
    }
  }

  /**
   * Keeps the body as a version now, and reads the list again in place: the
   * version kept is in it at once, and the one being read stays.
   */
  async keep(): Promise<void> {
    if (this.keeping) return;
    this.keeping = true;
    this.kept = undefined;
    try {
      const outcome = await keepVersion(this.#source);
      if (outcome !== 'failed' && outcome !== 'notNow' && outcome !== 'opening') {
        // A list that cannot be read now stays as it was: the version was kept all the same.
        this.#take(await this.#list().catch(() => this.versions));
      }
      this.kept = outcome;
    } finally {
      this.keeping = false;
    }
  }

  /** The list, with the names of who wrote known; what the reader typed is sent first. */
  async #list(): Promise<BodyVersion[]> {
    await this.#source.settle();
    const list = await this.#versions.list(this.#source.ownerId, this.#source.itemId);
    await this.#source.workspace.learnUsers(list.flatMap((one) => one.editors));
    return list;
  }

  /** A list read: the version being read is taken from it, for whether it is the card's body now. */
  #take(list: BodyVersion[]): void {
    this.versions = list;
    const chosen = this.chosen && list.find((one) => one.version === this.chosen?.version);
    if (!chosen) return;
    this.chosen = chosen;
    if (this.content) this.content = { ...this.content, isCurrent: chosen.isCurrent };
    // The version before it, or the one compared with, may be another now.
    void this.compare.refresh();
  }

  /**
   * Reads a version of the list; one chosen meanwhile takes its place. Not
   * while a version is being restored: its answer is about the one chosen.
   */
  async choose(version: BodyVersion): Promise<void> {
    if (this.restoring) return;
    this.chosen = version;
    this.content = undefined;
    this.contentStatus = 'loading';
    this.refusal = undefined;
    void this.compare.refresh();
    try {
      const content = await this.#versions.read(
        this.#source.ownerId,
        this.#source.itemId,
        version.version,
      );
      if (!this.#isChosen(version)) return;
      this.content = content;
      this.contentStatus = 'ready';
      void this.compare.refresh();
    } catch (error) {
      if (!this.#isChosen(version)) return;
      this.contentStatus = statusOf(error) === 404 ? 'gone' : 'failed';
    }
  }

  /**
   * Whether the version is still the one chosen, after a wait: by its number, for a list
   * read again meanwhile (`keep`) has the same version anew.
   */
  #isChosen(version: BodyVersion): boolean {
    return this.chosen?.version === version.version;
  }

  /** Back to the list. */
  leave(): void {
    if (this.restoring) return;
    this.chosen = undefined;
    this.content = undefined;
    this.refusal = undefined;
    void this.compare.refresh();
  }

  /** Writes the chosen version to the card again; whether the server took it. */
  async restore(): Promise<boolean> {
    const version = this.chosen;
    if (!version || this.restoring) return false;
    this.restoring = true;
    this.refusal = undefined;
    try {
      await this.#source.settle();
      await this.#versions.restore(this.#source.ownerId, this.#source.itemId, version.version);
      return true;
    } catch (error) {
      const status = statusOf(error);
      this.refusal = status === 404 ? 'gone' : status === 409 ? 'unrestorable' : 'failed';
      return false;
    } finally {
      this.restoring = false;
    }
  }
}

function statusOf(error: unknown): number {
  return isBackendError(error) ? error.status : 0;
}
