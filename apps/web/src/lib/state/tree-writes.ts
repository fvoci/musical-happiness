/**
 * The writes of an open wiki's tree (`wiki.svelte.ts`): making a page,
 * moving one with the pages below it, copying one with them, deleting one
 * with them and bringing one back from the trash. Each shows at once (bringing back, once it is
 * back) and goes back when the backend refuses it, told
 * (`workspace.report`); making, moving, copying and deleting are undone as
 * one step each, a deletion by bringing the page back, a copy by deleting it.
 *
 * A page deleted takes the pages below it along, and only it is told: the
 * pages below it are taken off here (`forget`), as of a deletion heard.
 */
import { isEmptyDoc } from '../editor/schema.ts';
import { isPlaced, moved, movesAnywhere, placeOf, type PagePlace } from '../logic/page-moves.ts';
import { withDescendants } from '../logic/page-tree.ts';

import {
  isBackendError,
  type PageDraft,
  type WikiPage,
  type WikiPageDetail,
  type Wikis,
} from './backend.ts';
import type { PageStore } from './page.svelte.ts';
import { unlessGone } from './wiki-parts.ts';
import { PendingWrites } from './write.ts';
import type { Workspace } from './workspace.svelte.ts';

/** What the writes of the tree need of the wiki's store. */
export interface TreeHost {
  readonly wikiId: string;
  readonly workspace: Workspace;
  wikis(): Wikis;
  /** Every page of the wiki, in no order: shown as the store's. */
  pages(): WikiPage[];
  setPages(pages: WikiPage[]): void;
  openPage(): PageStore | undefined;
  /** A page made or changed, as the backend gave it. */
  takePage(page: WikiPage): void;
  /** Reads the pages again, merged with what is shown. */
  reread(): void;
}

export class TreeWrites {
  readonly #host: TreeHost;

  constructor(host: TreeHost) {
    this.#host = host;
  }

  #page(pageId: string | undefined): WikiPage | undefined {
    return this.#host.pages().find((page) => page.id === pageId);
  }

  /**
   * Pages deleted, with every page below them. The page shown among them is
   * told so, unless the deletion is the reader's and not answered yet (`held`).
   */
  forget(pageIds: readonly string[], by: 'others' | 'reader' | 'held'): void {
    const gone = pageIds.flatMap((pageId) => [...withDescendants(this.#host.pages(), pageId)]);
    this.#forgettings += 1;
    for (const pageId of [...pageIds, ...gone]) {
      // The version it was shown at, or, gone already (its echo), the one kept then.
      const shownAt = this.#page(pageId)?.updatedAt ?? -Infinity;
      const updatedAt = Math.max(shownAt, this.#forgottenAt.get(pageId)?.updatedAt ?? -Infinity);
      this.#forgottenAt.set(pageId, { at: this.#forgettings, updatedAt });
    }
    const shown = this.#host.openPage();
    // The reader's own deletion, heard back before its answer: the reader's still.
    const own = pageIds.find((pageId) => this.#deleting.has(pageId));
    const title =
      own === undefined
        ? (this.#page(shown?.pageId)?.title ?? shown?.meta?.title)
        : this.#deleting.get(own);
    this.#host.setPages(this.#host.pages().filter((page) => !gone.includes(page.id)));
    if (by === 'held' || !shown || !gone.includes(shown.pageId)) return;
    shown.gone(own === undefined ? by : 'reader', title ?? '');
  }

  /** The pages the reader is deleting, each with the title of the page shown when it began. */
  readonly #deleting = new Map<string, string>();

  /** Whether the reader is deleting the page: it is off the tree by their doing, not told. */
  isDeleting(pageId: string): boolean {
    return this.#deleting.has(pageId);
  }

  /** Deletions taken off the tree so far (`forget`), counted. */
  #forgettings = 0;
  /** Each page taken off, with the count of deletions taken off then and its version then. */
  readonly #forgottenAt = new Map<string, { at: number; updatedAt: number }>();

  /** A mark of the deletions taken off so far: a reading begun now is told what went since. */
  get forgettings(): number {
    return this.#forgettings;
  }

  /** Whether the page was taken off the tree after `mark` (`forgettings`): a reading begun then still has it. */
  forgottenSince(mark: number, pageId: string): boolean {
    return (this.#forgottenAt.get(pageId)?.at ?? 0) > mark;
  }

  /**
   * Whether a page heard of is to be shown: not when the page it is below was
   * taken off the tree, which the backend tells alone (a copy undone at once,
   * its copies below heard after); it is then taken off too, and so are the
   * pages below it heard later; nor a page taken off itself, heard as it was
   * then (an echo come late). Brought back, a page is newer. A page shown is
   * no longer taken off.
   */
  admits(page: WikiPage): boolean {
    const forgotten = this.#forgottenAt.get(page.id);
    if (forgotten && page.updatedAt <= forgotten.updatedAt) return false;
    if (page.parentId !== '' && this.#forgottenAt.has(page.parentId)) {
      // Shown (a move of it under way, say), it goes now with the page it is below.
      if (this.#page(page.id)) this.forget([page.id], 'others');
      const kept = this.#forgottenAt.get(page.id)?.updatedAt ?? -Infinity;
      this.#forgottenAt.set(page.id, {
        at: this.#forgettings,
        updatedAt: Math.max(kept, page.updatedAt),
      });
      return false;
    }
    this.#forgottenAt.delete(page.id);
    return true;
  }

  /** The pages shown after a reading of the tree are there: none of them is taken off any more. */
  remember(pages: readonly WikiPage[]): void {
    for (const page of pages) this.#forgottenAt.delete(page.id);
  }

  /** The pages of a reading begun at `mark` that are to be shown, parents before what is below them. */
  admitted(pages: readonly WikiPage[], mark: number): WikiPage[] {
    const byId = new Map(pages.map((page) => [page.id, page]));
    const gone = (page: WikiPage): boolean => {
      const seen = new Set<string>();
      for (
        let at: WikiPage | undefined = page;
        at && !seen.has(at.id);
        at = byId.get(at.parentId)
      ) {
        seen.add(at.id);
        if (this.forgottenSince(mark, at.id)) return true;
        if (at.parentId !== '' && !byId.has(at.parentId) && this.#forgottenAt.has(at.parentId)) {
          return true;
        }
      }
      return false;
    };
    return pages.filter((page) => !gone(page));
  }

  /** The reader's deletions that the backend has not answered yet. */
  readonly #leaving = new Set<Promise<void>>();

  /**
   * Waits for the reader's deletions under way to be answered, kept or
   * refused: the trash read after it has what they took off the tree, which
   * is off it before the backend knows.
   */
  async deletionsAnswered(): Promise<void> {
    await Promise.allSettled([...this.#leaving]);
  }

  /** Deletes a page as the reader, its echo known for theirs (`forget`). */
  async #deletePage(
    pageId: string,
    title = this.#page(this.#host.openPage()?.pageId)?.title ?? '',
  ): Promise<void> {
    this.#deleting.set(pageId, title);
    const leaving = this.#host.wikis().deletePage(this.#host.wikiId, pageId);
    this.#leaving.add(leaving);
    try {
      await leaving;
    } finally {
      this.#leaving.delete(leaving);
      // After the echo, which comes before the answer or not at all.
      setTimeout(() => {
        this.#deleting.delete(pageId);
      }, 0);
    }
  }

  /**
   * Makes a page (`PageDraft`: below a page, after one, or last at the top)
   * and gives it, shown in the tree; undefined when it was refused (told).
   * Undoing it deletes it.
   */
  async createPage(draft: PageDraft): Promise<WikiPageDetail | undefined> {
    /** The page an undo kept (written in, or with pages below): done again, it is that page. */
    let kept: WikiPageDetail | undefined;
    try {
      return await this.#host.workspace.undo.perform(
        async () => {
          if (kept) {
            const again = kept;
            kept = undefined;
            return again;
          }
          const made = await this.#host.wikis().createPage(this.#host.wikiId, draft);
          this.#host.takePage(made);
          return made;
        },
        async (made) => {
          // Written in since, or with pages below it: kept, for undo would lose them for good.
          if (await this.#grownSince(made.id)) {
            kept = made;
            return;
          }
          await unlessGone(() => this.#deletePage(made.id));
          this.forget([made.id], 'reader');
        },
        'createPage',
      );
    } catch (error) {
      this.#host.workspace.report('save', error);
      return undefined;
    }
  }

  /** Whether a page has pages below it now, or a body: undoing its making would delete them. */
  async #grownSince(pageId: string): Promise<boolean> {
    if (this.#host.pages().some((page) => page.parentId === pageId)) return true;
    try {
      const { body } = await this.#host.wikis().page(this.#host.wikiId, pageId);
      return body !== null && !isEmptyDoc(body);
    } catch (error) {
      if (isBackendError(error) && error.status === 404) return false;
      throw error;
    }
  }

  /**
   * Deletes a page and every page below it: they are taken off the tree at
   * once and come back if the backend refuses. Undone, they come back from
   * the trash. False when it was refused (told).
   */
  async deletePage(pageId: string): Promise<boolean> {
    try {
      await this.#host.workspace.undo.perform(
        () => this.#deleteShown(pageId),
        // Brought back meanwhile (the trash, somebody else), or gone with a page above it:
        // nothing is left to undo.
        () => unlessGone(() => this.restorePage(pageId)),
        'deletePage',
      );
      return true;
    } catch (error) {
      this.#host.workspace.report('save', error);
      return false;
    }
  }

  /** Takes the page off the tree and deletes it; refused, it is put back, and the refusal thrown. */
  async #deleteShown(pageId: string): Promise<void> {
    const before = this.#host.pages();
    const gone = withDescendants(before, pageId);
    const title = this.#page(this.#host.openPage()?.pageId)?.title ?? '';
    const marks = [...gone].map((id) => [id, this.#forgottenAt.get(id)] as const);
    this.forget([pageId], 'held');
    try {
      await this.#deletePage(pageId, title);
    } catch (error) {
      // Not gone already (somebody else deleted it): it comes back as it was.
      if (!isBackendError(error) || error.status !== 404) {
        this.#host.setPages(before);
        // Not taken off after all: what is heard below it is shown again.
        for (const [id, mark] of marks) {
          if (mark) this.#forgottenAt.set(id, mark);
          else this.#forgottenAt.delete(id);
        }
        throw error;
      }
    }
    const shown = this.#host.openPage();
    if (shown && gone.has(shown.pageId)) shown.gone('reader', title);
  }

  /**
   * Copies a page with the pages below it, the copy titled `title`, right
   * after the page; gives the copy, undefined when it was refused (told).
   * Undone, the copies go to the trash; done again, they come back from it.
   */
  async duplicatePage(pageId: string, title: string): Promise<WikiPageDetail | undefined> {
    let copied: WikiPageDetail | undefined;
    try {
      return await this.#host.workspace.undo.perform(
        async () => {
          if (copied) {
            // Brought back meanwhile (the trash, somebody else), or gone for good: as it is.
            await unlessGone(() => this.restorePage(copied?.id ?? ''));
            return copied;
          }
          copied = await this.#host.wikis().duplicatePage(this.#host.wikiId, pageId, title);
          this.#host.takePage(copied);
          // The copies below it are told apart: until then, read the tree again.
          this.#host.reread();
          return copied;
        },
        (copy) => unlessGone(() => this.#deleteShown(copy.id)),
        'duplicatePage',
      );
    } catch (error) {
      this.#host.workspace.report('save', error);
      return undefined;
    }
  }

  /**
   * Brings a page back from the trash with the pages that went with it, and
   * shows them: the page at once, those below it as the tree is read again.
   * A refusal is thrown.
   */
  async restorePage(pageId: string): Promise<void> {
    const back = await this.#host.wikis().undeletePage(this.#host.wikiId, pageId);
    this.#host.takePage(back);
    this.#host.reread();
  }

  /**
   * Moves a page, with every page below it, where `to` says: shown at once,
   * undone as one step back to where it was. False when it was refused (told).
   * A page somebody else deleted meanwhile (404) is taken off the tree, as
   * when that is heard, and the reader is told why it did not move; a redo
   * over it has nothing left to do, and says nothing.
   */
  movePage(pageId: string, to: PagePlace): Promise<boolean> {
    // One after another, as asked for: each starts from where the one before left the
    // page, and goes back there when undone.
    // Nothing under way: shown at once.
    const turn =
      this.#queued === 0
        ? this.#movePage(pageId, to)
        : this.#queue.then(() => this.#movePage(pageId, to));
    this.#queued += 1;
    this.#queue = turn.finally(() => {
      this.#queued -= 1;
    });
    return turn;
  }

  async #movePage(pageId: string, to: PagePlace): Promise<boolean> {
    if (!movesAnywhere(this.#host.pages(), pageId, to)) return true;
    const back = placeOf(this.#host.pages(), pageId);
    if (!back) return false;
    let first = true;
    try {
      await this.#host.workspace.undo.perform(
        () => {
          if (!first) return unlessGone(() => this.#move(pageId, to));
          first = false;
          // Refused, the move leaves the history: there is nothing to undo.
          return this.#move(pageId, to);
        },
        () => unlessGone(() => this.#moveBack(pageId, back)),
        'movePage',
      );
      return true;
    } catch (error) {
      if (isBackendError(error) && error.status === 404) {
        this.forget([pageId], 'others');
        this.#host.workspace.report('movePage', error);
      } else this.#host.workspace.report('save', error);
      return false;
    }
  }

  /** The moves of a page under way, by page. */
  readonly #moving = new Map<string, number>();
  /** Versions of a page from the backend while a move of it is under way: held. */
  readonly #pending = new PendingWrites<WikiPage>();
  /** A page as it was shown before the first of its moves under way. */
  readonly #unmoved = new Map<string, WikiPage>();
  /** The moves asked for, one after another; each ends without rejecting. */
  #queue: Promise<unknown> = Promise.resolve();
  /** How many of them have not ended. */
  #queued = 0;

  /**
   * A version of a page from the backend (an answer, a change heard, a
   * reading): true to show it now, false while a move of it is under way and
   * it is held, to be shown when the last move comes back.
   */
  receive(page: WikiPage): boolean {
    return this.#pending.receive(page);
  }

  /**
   * Shows the page where `to` says and moves it there. Once its last move
   * under way (an undo's among them) comes back, the newest version the
   * backend gave of it is shown, or, all of them refused, the page as it was
   * before them. Rejects when this move was refused.
   */
  async #move(pageId: string, to: PagePlace): Promise<void> {
    const shown = this.#page(pageId);
    if (shown && !this.#moving.has(pageId)) this.#unmoved.set(pageId, shown);
    this.#moving.set(pageId, (this.#moving.get(pageId) ?? 0) + 1);
    this.#pending.begin(pageId);
    this.#host.setPages(moved(this.#host.pages(), pageId, to));
    let answer: WikiPage | undefined;
    try {
      answer = await this.#host.wikis().movePage(this.#host.wikiId, pageId, to);
    } finally {
      const latest = this.#pending.end(pageId, answer);
      const left = (this.#moving.get(pageId) ?? 1) - 1;
      if (left > 0) this.#moving.set(pageId, left);
      else {
        this.#moving.delete(pageId);
        const unmoved = this.#unmoved.get(pageId);
        this.#unmoved.delete(pageId);
        const show = latest ?? unmoved;
        if (show) this.#host.takePage(show);
        // Its siblings numbered anew are told of apart: until then, read the tree again.
        if (answer && !isPlaced(this.#host.pages(), pageId, to)) this.#host.reread();
      }
    }
  }

  /**
   * Moves the page back where it was: after the sibling it was after, or,
   * that one gone or moved away since, last below its parent; that one gone
   * too, last at the top.
   */
  async #moveBack(pageId: string, back: PagePlace): Promise<void> {
    try {
      await this.#move(pageId, back);
    } catch (error) {
      if (!isBackendError(error) || error.status !== 400) throw error;
      const parentRefused = error.refusalAt('/parentId') !== undefined;
      if (parentRefused && error.reason === 'page_too_deep') throw error;
      await this.#move(pageId, { parentId: parentRefused ? '' : back.parentId });
    }
  }
}
