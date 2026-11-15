/**
 * One open wiki: the wiki itself, its members (what the reader may do), and
 * its pages, a tree (`tree`, `logic/page-tree.ts`). The page that is shown is
 * a store of its own (`page.svelte.ts`), opened from here (`openPage`).
 *
 * The wiki is read as a board of the kind `wiki`: at the address of a board it
 * is not there, and a board is not there at a wiki's. Changes are heard as
 * the board's are (`watchBoard`): the wiki renamed or deleted, its members,
 * its pages made, changed and deleted (`pages`, `blocksDeleted`), and the
 * comments on the page shown (`blocks`, handed to `PageStore.comments`). A page
 * deleted takes the pages below it along, and only it is told: the store
 * takes them off itself.
 *
 * Writes show at once and go back when the backend refuses them, told
 * (`workspace.report`). Renaming a page, changing its icon, making one and
 * changing the wiki can be undone; deleting a page cannot (nothing brings one
 * back yet).
 */
import { isEmptyDoc } from '../editor/schema.ts';
import { pageTree, withDescendants } from '../logic/page-tree.ts';
import type { Board, BoardMember, MemberRole } from '../logic/wire.ts';

import {
  isBackendError,
  type Change,
  type PageDraft,
  type WikiDraft,
  type WikiPage,
  type WikiPageDetail,
  type Wikis,
} from './backend.ts';
import { boardOfWorkspace, roleOf } from './board-parts.ts';
import { isCommentBlock } from './changes.ts';
import { BoardPresence } from './board-presence.svelte.ts';
import { orderMembers } from './order.ts';
import { PageStore } from './page.svelte.ts';
import { memberId } from './user-names.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';
import { reread, take } from './write.ts';

/** What a page's title and icon are changed to: what is left out stays. */
export interface PageLooks {
  title?: string;
  icon?: string;
}

/** What the reader can change of the wiki itself. */
export interface WikiLooks {
  title?: string;
  description?: string;
  icon?: string;
}

export class WikiStore {
  readonly workspace: Workspace;
  readonly wikiId: string;

  status = $state<LoadStatus>('idle');
  /** The wiki is not there, or not for the reader: the two look the same. */
  missing = $state(false);
  /** The wiki is open and the reader no member of it: shown with a way to join (`join`). */
  joinable = $state(false);
  /** The reader asked to join and the answer has not come. */
  joining = $state(false);
  wiki = $state.raw<Board | undefined>(undefined);
  /** Every page of the wiki, in no order. */
  pages = $state.raw<WikiPage[]>([]);
  members = $state.raw<BoardMember[]>([]);
  /** The page shown, if one is. */
  openPage = $state.raw<PageStore | undefined>(undefined);

  /** The pages as a tree, each list of siblings in their order. */
  readonly tree = $derived(pageTree(this.pages));
  readonly orderedMembers = $derived.by(() =>
    orderMembers(
      this.members,
      (userId) => this.workspace.userName(userId) ?? '',
      this.workspace.locale,
    ),
  );
  /** What the reader is on this wiki. */
  readonly role = $derived<MemberRole>(
    roleOf(this.members.find((member) => member.userId === this.workspace.me?.id)),
  );
  /** Whether the reader writes pages: the wiki's admins and editors. */
  readonly canEdit = $derived(this.role === 'admin' || this.role === 'editor');
  /** Whether the reader changes and deletes the wiki itself: its admins. */
  readonly canManage = $derived(this.role === 'admin');

  #stopWatching: (() => void) | undefined;
  #closed = false;
  /** Readings begun (`load`, `#refresh`): the latest is the one that counts. */
  #readings = 0;

  /** Who else is on the wiki, and on which page: started while it is shown (`start`). */
  readonly presence: BoardPresence;

  constructor(workspace: Workspace, wikiId: string) {
    this.workspace = workspace;
    this.wikiId = wikiId;
    this.presence = new BoardPresence({ workspace, boardId: wikiId });
  }

  get #wikis(): Wikis {
    // Opened only where the backend has wikis (the app's, not a public link's).
    const wikis = this.workspace.backend.wiki;
    if (!wikis) throw new Error('this backend has no wikis');
    return wikis;
  }

  page(pageId: string | undefined): WikiPage | undefined {
    return this.pages.find((page) => page.id === pageId);
  }

  async load(): Promise<void> {
    this.status = 'loading';
    this.missing = false;
    this.joinable = false;
    this.#readings += 1;
    const reading = this.#readings;
    try {
      // Listen first: a change made while the lists are being read is not lost.
      this.#stopWatching?.();
      this.#stopWatching = this.workspace.backend.watchBoard(this.wikiId, (change) => {
        this.#apply(change);
      });
      const backend = this.workspace.backend;
      const [wiki, pages, members] = await Promise.all([
        boardOfWorkspace(backend, this.wikiId, this.workspace.info?.id, 'wiki'),
        this.#wikis.pages(this.wikiId),
        backend.members(this.wikiId),
      ]);
      if (this.#closed || reading !== this.#readings) return;
      void this.workspace.learnUsers(members.map(memberId), { members: true });
      [this.wiki, this.pages, this.members] = [wiki, pages, members];
      this.status = 'ready';
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      // An open wiki to join is told before the reading ends: no failure shows in between.
      if (isBackendError(error) && error.status === 403) await this.#explainRefusal();
      else if (isBackendError(error) && error.status === 404) this.missing = true;
      else this.workspace.report('load', error);
      if (!this.#isClosed() && reading === this.#readings) this.status = 'failed';
    }
  }

  /**
   * A refusal (403) of an open wiki of the workspace is one the reader may
   * join: it reads as itself, with a way to join, while its pages are
   * refused. Anything else is not there for them.
   */
  async #explainRefusal(): Promise<void> {
    try {
      const wiki = await boardOfWorkspace(
        this.workspace.backend,
        this.wikiId,
        this.workspace.info?.id,
        'wiki',
      );
      if (this.#closed) return;
      this.wiki = wiki;
      this.joinable = wiki.type === 'O';
      this.missing = !this.joinable;
    } catch (error) {
      this.missing = true;
      if (!isBackendError(error) || error.status !== 404) this.workspace.report('load', error);
    }
  }

  /** Joins the open wiki shown (`joinable`) and reads it. False (and told) when that failed. */
  async join(): Promise<boolean> {
    if (this.joining) return false;
    this.joining = true;
    try {
      await this.workspace.backend.joinBoard(this.wikiId);
    } catch (error) {
      // Made private or deleted meanwhile: there is nothing to join.
      if (isBackendError(error) && error.status === 404) {
        [this.joinable, this.missing] = [false, true];
      } else this.workspace.report('save', error);
      return false;
    } finally {
      this.joining = false;
    }
    if (this.#closed) return false;
    await this.load();
    return this.status === 'ready';
  }

  /** Asked as a function: `#closed` may change while a reading waits, which TypeScript does not see. */
  #isClosed(): boolean {
    return this.#closed;
  }

  close(): void {
    this.#closed = true;
    this.#stopWatching?.();
    this.#stopWatching = undefined;
    this.openPage?.close();
    this.openPage = undefined;
  }

  /** Shows a page: its store is made, and the one before closed. None for `undefined`. */
  showPage(pageId: string | undefined): PageStore | undefined {
    // The same page, unless it was found gone: shown again, it is read anew (restored, let back in).
    const open = this.openPage;
    if (open && open.pageId === pageId && !open.missing) return open;
    this.openPage?.close();
    this.openPage = pageId === undefined ? undefined : new PageStore(this, pageId);
    void this.openPage?.load();
    return this.openPage;
  }

  // ------------------------------------------------------------ changes

  #apply(change: Change): void {
    switch (change.kind) {
      case 'board':
        if (change.board.id !== this.wikiId) break;
        // Deleted and brought back, or joined: everything of it is read again.
        if (this.missing || this.joinable) void this.load();
        else if (!this.wiki || this.wiki.updateAt <= change.board.updateAt) {
          this.wiki = change.board;
        }
        break;
      case 'boardDeleted':
        if (change.boardId !== this.wikiId) break;
        // An open wiki may only have been closed to the reader: read again, it may be one to join.
        if (this.wiki?.type === 'O' && !this.#closed) void this.load();
        else [this.missing, this.status] = [true, 'failed'];
        break;
      case 'pages':
        for (const page of change.pages) this.#takePage(page);
        break;
      case 'blocks':
        for (const block of change.blocks)
          if (isCommentBlock(block)) this.openPage?.comments.receive(block);
        break;
      case 'blocksDeleted':
        this.#forget(change.blockIds, 'others');
        this.openPage?.comments.forget(change.blockIds);
        break;
      case 'members':
        if (change.boardId !== this.wikiId) break;
        this.members = change.members;
        void this.workspace.learnUsers(change.members.map(memberId), { members: true });
        break;
      case 'resync':
        void this.#refresh();
        break;
      default:
        break;
    }
  }

  /** A page made or changed: the newer of the two is kept, and the open page is told. */
  #takePage(page: WikiPage): void {
    if (page.wikiId !== this.wikiId) return;
    this.pages = take(this.pages, page);
    if (this.openPage?.pageId === page.id) this.openPage.pageChanged(page);
  }

  /**
   * Pages deleted, with every page below them. The page shown among them is
   * told so, unless the deletion is the reader's and not answered yet (`held`).
   */
  #forget(pageIds: readonly string[], by: 'others' | 'reader' | 'held'): void {
    const gone = pageIds.flatMap((pageId) => [...withDescendants(this.pages, pageId)]);
    const shown = this.openPage;
    // The reader's own deletion, heard back before its answer: the reader's still.
    const own = pageIds.find((pageId) => this.#deleting.has(pageId));
    const title = own === undefined ? this.page(shown?.pageId)?.title : this.#deleting.get(own);
    this.pages = this.pages.filter((page) => !gone.includes(page.id));
    if (by === 'held' || !shown || !gone.includes(shown.pageId)) return;
    shown.gone(own === undefined ? by : 'reader', title ?? '');
  }

  /** The pages the reader is deleting, each with the title of the page shown when it began. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- consulted, never shown
  readonly #deleting = new Map<string, string>();

  /** Deletes a page as the reader, its echo known for theirs (`#forget`). */
  async #deletePage(
    pageId: string,
    title = this.page(this.openPage?.pageId)?.title ?? '',
  ): Promise<void> {
    this.#deleting.set(pageId, title);
    try {
      await this.#wikis.deletePage(this.wikiId, pageId);
    } finally {
      // After the echo, which comes before the answer or not at all.
      setTimeout(() => {
        this.#deleting.delete(pageId);
      }, 0);
    }
  }

  /**
   * Reads the wiki, its members and pages again after changes may have been
   * missed (`resync`): the pages merged with what is shown (`reread`).
   */
  async #refresh(): Promise<void> {
    if (this.status !== 'ready' || this.#closed) return;
    this.#readings += 1;
    const reading = this.#readings;
    const before = this.pages.map((page) => page.id);
    try {
      const [wiki, pages, members] = await Promise.all([
        this.workspace.backend.board(this.wikiId),
        this.#wikis.pages(this.wikiId),
        this.workspace.backend.members(this.wikiId),
      ]);
      if (this.#isClosed() || reading !== this.#readings) return;
      [this.wiki, this.members] = [wiki, members];
      const shown = this.openPage;
      const title = (shown && this.page(shown.pageId)?.title) ?? '';
      this.pages = reread(this.pages, pages, before);
      if (shown && !this.page(shown.pageId)) shown.gone('others', title);
      else if (shown) void shown.refresh();
    } catch (error) {
      if (this.#isClosed() || reading !== this.#readings) return;
      // Gone for the reader (deleted, or taken off it): read as on opening.
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        void this.load();
      }
    }
  }

  // ------------------------------------------------------------- writes

  /**
   * Makes a page (`PageDraft`: below a page, after one, or last at the top)
   * and gives it, shown in the tree; undefined when it was refused (told).
   * Undoing it deletes it.
   */
  async createPage(draft: PageDraft): Promise<WikiPageDetail | undefined> {
    /** The page an undo kept (written in, or with pages below): done again, it is that page. */
    let kept: WikiPageDetail | undefined;
    try {
      return await this.workspace.undo.perform(
        async () => {
          if (kept) {
            const again = kept;
            kept = undefined;
            return again;
          }
          const made = await this.#wikis.createPage(this.wikiId, draft);
          this.#takePage(made);
          return made;
        },
        async (made) => {
          // Written in since, or with pages below it: kept, for undo would lose them for good.
          if (await this.#grownSince(made.id)) {
            kept = made;
            return;
          }
          await unlessGone(() => this.#deletePage(made.id));
          this.#forget([made.id], 'reader');
        },
        'createPage',
      );
    } catch (error) {
      this.workspace.report('save', error);
      return undefined;
    }
  }

  /** Whether a page has pages below it now, or a body: undoing its making would delete them. */
  async #grownSince(pageId: string): Promise<boolean> {
    if (this.pages.some((page) => page.parentId === pageId)) return true;
    try {
      const { body } = await this.#wikis.page(this.wikiId, pageId);
      return body !== null && !isEmptyDoc(body);
    } catch (error) {
      if (isBackendError(error) && error.status === 404) return false;
      throw error;
    }
  }

  /** Changes the title or the icon of a page: shown at once, undone as one step. */
  async changePage(pageId: string, looks: PageLooks): Promise<void> {
    const before = this.page(pageId);
    if (!before) return;
    const back: PageLooks = {
      ...(looks.title === undefined ? {} : { title: before.title }),
      ...(looks.icon === undefined ? {} : { icon: before.icon }),
    };
    if (Object.entries(looks).every(([key, value]) => back[key as keyof PageLooks] === value)) {
      return;
    }
    try {
      await this.workspace.undo.perform(
        () => unlessGone(() => this.#writeLooks(pageId, looks, back)),
        () => unlessGone(() => this.#writeLooks(pageId, back, looks)),
        looks.title === undefined ? 'changePageIcon' : 'changePageTitle',
      );
    } catch (error) {
      this.workspace.report('save', error);
    }
  }

  /** Shows `looks` on the page and writes them; on a refusal shows `back` again and rejects. */
  async #writeLooks(pageId: string, looks: PageLooks, back: PageLooks): Promise<void> {
    const shown = (with_: PageLooks): void => {
      this.pages = this.pages.map((page) => (page.id === pageId ? { ...page, ...with_ } : page));
    };
    shown(looks);
    try {
      this.#takePage(await this.#wikis.patchPage(this.wikiId, pageId, looks));
    } catch (error) {
      shown(back);
      throw error;
    }
  }

  /**
   * Deletes a page and every page below it: they are taken off the tree at
   * once and come back if the backend refuses. Not undone: nothing brings a
   * page back yet.
   */
  async deletePage(pageId: string): Promise<boolean> {
    const before = this.pages;
    const gone = withDescendants(before, pageId);
    const title = this.page(this.openPage?.pageId)?.title ?? '';
    this.#forget([pageId], 'held');
    try {
      await this.#deletePage(pageId, title);
    } catch (error) {
      // Not gone already (somebody else deleted it): it comes back as it was.
      if (!isBackendError(error) || error.status !== 404) {
        this.pages = before;
        this.workspace.report('save', error);
        return false;
      }
    }
    if (this.openPage && gone.has(this.openPage.pageId)) this.openPage.gone('reader', title);
    return true;
  }

  /** Changes the wiki's title, description or icon: shown at once, undone as one step. */
  async changeWiki(looks: WikiLooks): Promise<void> {
    const before = this.wiki;
    if (!before) return;
    const back: WikiLooks = {
      ...(looks.title === undefined ? {} : { title: before.title }),
      ...(looks.description === undefined ? {} : { description: before.description }),
      ...(looks.icon === undefined ? {} : { icon: before.icon ?? '' }),
    };
    const write = async (to: WikiLooks, from: WikiLooks): Promise<void> => {
      if (this.wiki) this.wiki = { ...this.wiki, ...to };
      try {
        const changed = await this.workspace.backend.patchBoard(this.wikiId, to);
        if (!this.wiki || this.wiki.updateAt <= changed.updateAt) this.wiki = changed;
        this.workspace.receiveBoard(changed);
      } catch (error) {
        if (this.wiki) this.wiki = { ...this.wiki, ...from };
        throw error;
      }
    };
    try {
      await this.workspace.undo.perform(
        () => write(looks, back),
        () => write(back, looks),
        'changeWiki',
      );
    } catch (error) {
      this.workspace.report('save', error);
    }
  }
}

/** A step of undo on a page deleted since (by the reader too): nothing is left to do. */
async function unlessGone(step: () => Promise<void>): Promise<void> {
  try {
    await step();
  } catch (error) {
    if (!isBackendError(error) || error.status !== 404) throw error;
  }
}

/**
 * Makes a wiki in the workspace shown, in the sidebar at once; undefined when
 * it was refused (told) or the backend has no wikis.
 */
export async function createWiki(
  workspace: Workspace,
  draft: WikiDraft,
): Promise<Board | undefined> {
  const wikis = workspace.backend.wiki;
  const shown = workspace.info;
  if (!wikis || !shown) return undefined;
  try {
    const made = await wikis.createWiki(shown.id, draft);
    workspace.receiveBoard(made);
    return made;
  } catch (error) {
    workspace.report('save', error);
    return undefined;
  }
}
