/**
 * One open wiki: the wiki itself, its members (what the reader may do), and
 * its pages, a tree (`tree`, `logic/page-tree.ts`). The page that is shown is
 * a store of its own (`page.svelte.ts`), opened from here (`openPage`).
 *
 * The wiki is read as a board of the kind `wiki`: at the address of a board it
 * is not there, and a board is not there at a wiki's. Changes are heard as
 * the board's are (`watchContainer`): the wiki renamed or deleted, its members,
 * its pages made, changed and deleted (`pages`, `blocksDeleted`), and the
 * comments on the page shown (`blocks`, handed to `PageStore.comments`). A page
 * deleted takes the pages below it along, and only it is told: the store
 * takes them off itself.
 *
 * Writes show at once and go back when the backend refuses them, told
 * (`workspace.report`). Renaming a page, changing its icon, making one,
 * moving one, copying one, deleting one (brought back from the trash) and
 * changing the wiki can be undone.
 *
 * An open wiki is read by whoever works in the workspace without joining it
 * (`joinable`): they join it to write. Its people and its openness are changed
 * through the dialogs a board's are (`containerId`, `board`, `members`).
 */
import type { PagePlace } from '../logic/page-moves.ts';
import { pageTree } from '../logic/page-tree.ts';
import type { Container, ContainerMember, MemberRole } from '../logic/wire.ts';

import {
  isBackendError,
  type ContainerRole,
  type Change,
  type DeletedItem,
  type PageDraft,
  type Paging,
  type WikiPage,
  type WikiPageDetail,
  type Wikis,
} from './backend.ts';
import { containerOfWorkspace, roleOf } from './board-parts.ts';
import { isCommentBlock } from './changes.ts';
import { MemberWrites } from './member-writes.ts';
import { ContainerPresence } from './board-presence.svelte.ts';
import { orderMembers } from './order.ts';
import { PageStore } from './page.svelte.ts';
import { TreeWrites } from './tree-writes.ts';
import { unlessGone } from './wiki-parts.ts';
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
  /** Open to the workspace (`O`) or for the invited only (`P`): its admins'. */
  type?: Container['type'];
  /** The role of who joins it: its admins'. */
  minimumRole?: Container['minimumRole'];
}

export class WikiStore {
  readonly workspace: Workspace;
  readonly wikiId: string;

  status = $state<LoadStatus>('idle');
  /** The wiki is not there, or not for the reader: the two look the same. */
  missing = $state(false);
  /** The reader asked to join and the answer has not come. */
  joining = $state(false);
  wiki = $state.raw<Container | undefined>(undefined);
  /** Every page of the wiki, in no order. */
  pages = $state.raw<WikiPage[]>([]);
  members = $state.raw<ContainerMember[]>([]);
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
  /**
   * Whether the reader changes and deletes the wiki itself: its admins. An
   * administrator of the workspace who manages an open wiki without being its
   * member joins it first, as on an open board.
   */
  readonly canManage = $derived(this.role === 'admin');
  /**
   * The wiki is open and the reader no member of it: they read it, and join it
   * (`join`) to write in it.
   */
  readonly joinable = $derived(
    this.status === 'ready' && this.wiki?.type === 'O' && this.role === '',
  );

  #stopWatching: (() => void) | undefined;
  #closed = false;
  /** Readings begun (`load`, `#refresh`): the latest is the one that counts. */
  #readings = 0;

  /** Who else is on the wiki, and on which page: started while it is shown (`start`). */
  readonly presence: ContainerPresence;
  /** Making, moving and deleting pages (`tree-writes.ts`). */
  readonly #writes: TreeWrites;
  /** Adding, changing and taking off members (`member-writes.ts`). */
  readonly #people = new MemberWrites(this);

  constructor(workspace: Workspace, wikiId: string) {
    this.workspace = workspace;
    this.wikiId = wikiId;
    this.presence = new ContainerPresence({ workspace, containerId: wikiId });
    this.#writes = new TreeWrites({
      wikiId,
      workspace,
      wikis: () => this.#wikis,
      pages: () => this.pages,
      setPages: (pages) => {
        this.pages = pages;
      },
      openPage: () => this.openPage,
      takePage: (page) => {
        this.#takePage(page);
      },
      reread: () => {
        void this.#refresh();
      },
    });
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
    this.#readings += 1;
    const reading = this.#readings;
    const membersTold = this.#people.told;
    try {
      // Listen first: a change made while the lists are being read is not lost.
      this.#stopWatching?.();
      this.#stopWatching = this.workspace.backend.watchContainer(this.wikiId, (change) => {
        this.#apply(change);
      });
      const backend = this.workspace.backend;
      const [wiki, pages, members] = await Promise.all([
        containerOfWorkspace(backend, this.wikiId, this.workspace.info?.id, 'wiki'),
        this.#wikis.pages(this.wikiId),
        backend.members(this.wikiId),
      ]);
      if (this.#closed || reading !== this.#readings) return;
      void this.workspace.learnUsers(members.map(memberId), { members: true });
      this.#takeWiki(wiki);
      this.pages = pages;
      // Members told meanwhile are newer than this reading.
      if (membersTold === this.#people.told) this.members = members;
      this.status = 'ready';
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      // Not there for the reader, or not to read: the two look the same.
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.missing = true;
      } else this.workspace.report('load', error);
      this.status = 'failed';
    }
  }

  /** Joins the open wiki shown (`joinable`) and reads it. False (and told) when that failed. */
  async join(): Promise<boolean> {
    if (this.joining) return false;
    this.joining = true;
    try {
      await this.workspace.backend.joinContainer(this.wikiId);
    } catch (error) {
      // Made private or deleted meanwhile: there is nothing to join.
      if (isBackendError(error) && error.status === 404) this.missing = true;
      else this.workspace.report('save', error);
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

  /** Shows `wiki` unless a newer one is shown: a reading may land after a change told meanwhile. */
  #takeWiki(wiki: Container): void {
    if (!this.wiki || this.wiki.updateAt <= wiki.updateAt) this.wiki = wiki;
  }

  #apply(change: Change): void {
    switch (change.kind) {
      case 'container':
        if (change.container.id !== this.wikiId) break;
        // Deleted and brought back, or joined: everything of it is read again.
        if (this.missing) void this.load();
        else this.#takeWiki(change.container);
        break;
      case 'containerDeleted':
        if (change.containerId !== this.wikiId) break;
        // An open wiki may only have been closed to the reader: read again, it may be one to join.
        if (this.wiki?.type === 'O' && !this.#closed) void this.load();
        else [this.missing, this.status] = [true, 'failed'];
        break;
      case 'pages':
        for (const page of change.pages) this.#receive(page);
        break;
      case 'blocks':
        for (const block of change.blocks)
          if (isCommentBlock(block)) this.openPage?.comments.receive(block);
        break;
      case 'blocksDeleted':
        this.#writes.forget(change.blockIds, 'others');
        this.openPage?.comments.forget(change.blockIds);
        break;
      case 'members':
        if (change.containerId !== this.wikiId) break;
        this.#people.told += 1;
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

  /** A version of a page from the backend: shown, unless a move of it is under way. */
  #receive(page: WikiPage): void {
    if (this.#writes.receive(page)) this.#takePage(page);
  }

  /** A page made or changed: the newer of the two is kept, and the open page is told. */
  #takePage(page: WikiPage): void {
    if (page.wikiId !== this.wikiId || !this.#writes.admits(page)) return;
    this.pages = take(this.pages, page);
    const shown = this.openPage;
    if (shown?.pageId !== page.id) return;
    // Brought back while shown as not there: read again.
    if (shown.missing) void shown.load();
    else shown.pageChanged(page);
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
    const mark = this.#writes.forgettings;
    const membersTold = this.#people.told;
    try {
      const [wiki, pages, members] = await Promise.all([
        this.workspace.backend.container(this.wikiId),
        this.#wikis.pages(this.wikiId),
        this.workspace.backend.members(this.wikiId),
      ]);
      if (this.#isClosed() || reading !== this.#readings) return;
      this.#takeWiki(wiki);
      // Members told meanwhile are newer than this reading.
      if (membersTold === this.#people.told) this.members = members;
      const shown = this.openPage;
      const title = (shown && this.page(shown.pageId)?.title) ?? '';
      // What was taken off while it was read is gone, though the reading has it.
      const read = this.#writes.admitted(pages, mark);
      this.pages = reread(this.pages, read, before, (page) => this.#writes.receive(page));
      this.#writes.remember(this.pages);
      // The reader's own deletion under way tells of itself.
      if (shown && !this.page(shown.pageId)) {
        if (!this.#writes.isDeleting(shown.pageId)) shown.gone('others', title);
      } else if (shown) void shown.refresh();
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
   */
  createPage(draft: PageDraft): Promise<WikiPageDetail | undefined> {
    return this.#writes.createPage(draft);
  }

  /** The reader's moves the backend has not answered yet: the tree says it is settling, as a board does. */
  moving = $state(0);

  /** Moves a page with the pages below it where `to` says; false when it was refused (told). */
  async movePage(pageId: string, to: PagePlace): Promise<boolean> {
    this.moving += 1;
    try {
      return await this.#writes.movePage(pageId, to);
    } finally {
      this.moving -= 1;
    }
  }

  /** Copies a page with the pages below it, right after it; undefined when it was refused (told). */
  duplicatePage(pageId: string, title: string): Promise<WikiPageDetail | undefined> {
    return this.#writes.duplicatePage(pageId, title);
  }

  /** Deletes a page and every page below it; false when it was refused (told). */
  deletePage(pageId: string): Promise<boolean> {
    return this.#writes.deletePage(pageId);
  }

  /**
   * The wiki's pages in the trash, for its editors: a page of them, the last
   * deleted first, those the reader is deleting right now among them.
   */
  async deletedPages(paging?: Paging): Promise<DeletedItem[]> {
    await this.#writes.deletionsAnswered();
    return this.#wikis.deletedPages(this.wikiId, paging);
  }

  /** Brings a page back from the trash with those that went with it; a refusal is thrown. */
  restorePage(pageId: string): Promise<void> {
    return this.#writes.restorePage(pageId);
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
      this.#receive(await this.#wikis.patchPage(this.wikiId, pageId, looks));
    } catch (error) {
      shown(back);
      throw error;
    }
  }

  /**
   * Changes the wiki's title, description, icon, openness or the role of who
   * joins it: shown at once, undone as one step (`step` names it).
   */
  async changeWiki(
    looks: WikiLooks,
    step: 'changeWiki' | 'changeBoardType' | 'changeMinimumRole' = 'changeWiki',
  ): Promise<void> {
    const before = this.wiki;
    if (!before) return;
    const back: WikiLooks = {
      ...(looks.title === undefined ? {} : { title: before.title }),
      ...(looks.description === undefined ? {} : { description: before.description }),
      ...(looks.icon === undefined ? {} : { icon: before.icon ?? '' }),
      ...(looks.type === undefined ? {} : { type: before.type }),
      ...(looks.minimumRole === undefined ? {} : { minimumRole: before.minimumRole }),
    };
    const write = async (to: WikiLooks, from: WikiLooks): Promise<void> => {
      if (this.wiki) this.wiki = { ...this.wiki, ...to };
      try {
        const changed = await this.workspace.backend.patchContainer(this.wikiId, to);
        this.#takeWiki(changed);
        this.workspace.receiveContainer(changed);
      } catch (error) {
        if (this.wiki) this.wiki = { ...this.wiki, ...from };
        throw error;
      }
    };
    try {
      await this.workspace.undo.perform(
        () => write(looks, back),
        () => write(back, looks),
        step,
      );
    } catch (error) {
      this.workspace.report('save', error);
    }
  }

  // ------------------------------------------------------------ sharing
  // What the dialogs of a board's people read and change (`ShareDialog`).

  get containerId(): string {
    return this.wikiId;
  }

  get container(): Container | undefined {
    return this.wiki;
  }

  /** The wiki as read for whoever manages it without reading it (`managed-container.ts`). */
  set container(wiki: Container | undefined) {
    this.wiki = wiki;
  }

  /** Whether anybody of the workspace reads the wiki, and may join it to write. */
  setOpen(open: boolean): Promise<void> {
    return this.changeWiki({ type: open ? 'O' : 'P' }, 'changeBoardType');
  }

  /** The role somebody gets by joining the wiki. Members keep theirs. */
  setMinimumRole(role: ContainerRole): Promise<void> {
    return this.changeWiki({ minimumRole: role }, 'changeMinimumRole');
  }

  addMember(userId: string, role: ContainerRole): Promise<void> {
    return this.#people.add(userId, role);
  }

  setMemberRole(userId: string, role: ContainerRole): Promise<void> {
    return this.#people.setRole(userId, role);
  }

  removeMember(userId: string): Promise<void> {
    return this.#people.remove(userId);
  }
}
