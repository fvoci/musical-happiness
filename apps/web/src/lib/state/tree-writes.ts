/**
 * The writes of an open wiki's tree (`wiki.svelte.ts`): making a page,
 * moving one with the pages below it, and deleting one with them. Each shows
 * at once and goes back when the backend refuses it, told
 * (`workspace.report`); making and moving are undone as one step each.
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
    const shown = this.#host.openPage();
    // The reader's own deletion, heard back before its answer: the reader's still.
    const own = pageIds.find((pageId) => this.#deleting.has(pageId));
    const title = own === undefined ? this.#page(shown?.pageId)?.title : this.#deleting.get(own);
    this.#host.setPages(this.#host.pages().filter((page) => !gone.includes(page.id)));
    if (by === 'held' || !shown || !gone.includes(shown.pageId)) return;
    shown.gone(own === undefined ? by : 'reader', title ?? '');
  }

  /** The pages the reader is deleting, each with the title of the page shown when it began. */
  readonly #deleting = new Map<string, string>();

  /** Deletes a page as the reader, its echo known for theirs (`forget`). */
  async #deletePage(
    pageId: string,
    title = this.#page(this.#host.openPage()?.pageId)?.title ?? '',
  ): Promise<void> {
    this.#deleting.set(pageId, title);
    try {
      await this.#host.wikis().deletePage(this.#host.wikiId, pageId);
    } finally {
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
   * once and come back if the backend refuses. Not undone: nothing brings a
   * page back yet.
   */
  async deletePage(pageId: string): Promise<boolean> {
    const before = this.#host.pages();
    const gone = withDescendants(before, pageId);
    const title = this.#page(this.#host.openPage()?.pageId)?.title ?? '';
    this.forget([pageId], 'held');
    try {
      await this.#deletePage(pageId, title);
    } catch (error) {
      // Not gone already (somebody else deleted it): it comes back as it was.
      if (!isBackendError(error) || error.status !== 404) {
        this.#host.setPages(before);
        this.#host.workspace.report('save', error);
        return false;
      }
    }
    const shown = this.#host.openPage();
    if (shown && gone.has(shown.pageId)) shown.gone('reader', title);
    return true;
  }

  /**
   * Moves a page, with every page below it, where `to` says: shown at once,
   * undone as one step back to where it was. False when it was refused (told).
   */
  async movePage(pageId: string, to: PagePlace): Promise<boolean> {
    if (!movesAnywhere(this.#host.pages(), pageId, to)) return true;
    const back = placeOf(this.#host.pages(), pageId);
    if (!back) return false;
    try {
      await this.#host.workspace.undo.perform(
        () => unlessGone(() => this.#move(pageId, to)),
        () => unlessGone(() => this.#moveBack(pageId, back)),
        'movePage',
      );
      return true;
    } catch (error) {
      this.#host.workspace.report('save', error);
      return false;
    }
  }

  /** Shows the page where `to` says and moves it there; on a refusal it is shown back and rejects. */
  async #move(pageId: string, to: PagePlace): Promise<void> {
    const was = this.#page(pageId);
    this.#host.setPages(moved(this.#host.pages(), pageId, to));
    try {
      this.#host.takePage(await this.#host.wikis().movePage(this.#host.wikiId, pageId, to));
      // Its siblings numbered anew are told of apart: until then, read the tree again.
      if (!isPlaced(this.#host.pages(), pageId, to)) this.#host.reread();
    } catch (error) {
      if (was)
        this.#host.setPages(this.#host.pages().map((page) => (page.id === pageId ? was : page)));
      throw error;
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
