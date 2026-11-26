/**
 * One open wiki: the wiki itself, its members, its tree of pages (`pageTree`),
 * and the currently open `PageStore`. Optimistic hierarchy writes are handled
 * by `HierarchyWriteQueue`.
 */
import { isEmptyDoc } from '../editor/schema.ts';
import type { PagePlace } from '../logic/page-moves.ts';
import { pageTree } from '../logic/page-tree.ts';
import type { Container, ContainerMember, MemberRole } from '../logic/wire.ts';

import {
  isBackendError,
  type Change,
  type ContainerRole,
  type DeletedItem,
  type PageDraft,
  type Paging,
  type WikiPage,
  type WikiPageDetail,
  type Wikis,
} from './backend.ts';
import { containerOfWorkspace, roleOf } from './board-parts.ts';
import { ContainerPresence } from './board-presence.svelte.ts';
import { isCommentBlock } from './changes.ts';
import { HierarchyWriteQueue } from './hierarchy-writes.ts';
import { MemberWrites } from './member-writes.ts';
import { orderMembers } from './order.ts';
import { PageStore } from './page.svelte.ts';
import { memberId } from './user-names.ts';
import { joinOpenWiki, performPageLooks, performWikiLooks } from './wiki-parts.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';
import { reread, take } from './write.ts';

export interface PageLooks {
  title?: string;
  icon?: string;
}

export interface WikiLooks {
  title?: string;
  description?: string;
  icon?: string;
  visibility?: Container['visibility'];
  minimumRole?: Container['minimumRole'];
}

export class WikiStore {
  readonly kind = 'wiki' as const;
  readonly workspace: Workspace;
  readonly wikiId: string;

  status = $state<LoadStatus>('idle');
  missing = $state(false);
  pagesReady = $state(false);
  speculativePageId = $state<string | undefined>(undefined);
  joining = $state(false);
  wiki = $state.raw<Container | undefined>(undefined);
  pages = $state.raw<WikiPage[]>([]);
  members = $state.raw<ContainerMember[]>([]);
  openPage = $state.raw<PageStore | undefined>(undefined);

  readonly tree = $derived(pageTree(this.pages));
  readonly orderedMembers = $derived.by(() =>
    orderMembers(
      this.members,
      (userId) => this.workspace.userName(userId) ?? '',
      this.workspace.locale,
    ),
  );
  readonly role = $derived<MemberRole>(
    roleOf(this.members.find((member) => member.userId === this.workspace.me?.id)),
  );
  readonly canEdit = $derived(this.role === 'admin' || this.role === 'editor');
  readonly canManage = $derived(this.role === 'admin');
  readonly joinable = $derived(
    this.status === 'ready' && this.wiki?.visibility === 'open' && this.role === '',
  );

  #stopWatching: (() => void) | undefined;
  #closed = false;
  #readings = 0;

  readonly presence: ContainerPresence;
  readonly #writes: HierarchyWriteQueue<WikiPage, WikiPageDetail, PageDraft>;
  readonly #people = new MemberWrites(this);

  constructor(workspace: Workspace, wikiId: string) {
    this.workspace = workspace;
    this.wikiId = wikiId;
    this.presence = new ContainerPresence({ workspace, containerId: wikiId });
    this.#writes = new HierarchyWriteQueue({
      workspace,
      nodes: () => this.pages,
      setNodes: (pages) => {
        this.pages = pages;
      },
      openNode: () => this.openPage,
      takeNode: (page) => {
        this.#takePage(page);
      },
      reread: () => {
        void this.#refresh();
      },
      create: (draft) => this.#wikis.createPage(wikiId, draft),
      hasBody: async (pageId) => {
        const { body } = await this.#wikis.page(wikiId, pageId);
        return body !== null && !isEmptyDoc(body);
      },
      remove: (pageId) => this.#wikis.deletePage(wikiId, pageId),
      restore: (pageId) => this.#wikis.restorePage(wikiId, pageId),
      move: (pageId, to) => this.#wikis.movePage(wikiId, pageId, to),
      duplicate: (pageId, title) => this.#wikis.duplicatePage(wikiId, pageId, title),
    });
  }

  get #wikis(): Wikis {
    const wikis = this.workspace.backend.wiki;
    if (!wikis) throw new Error('this backend has no wikis');
    return wikis;
  }

  page(pageId: string | undefined): WikiPage | undefined {
    return this.pages.find((page) => page.id === pageId);
  }

  async load(options: { pageId?: string; speculative?: boolean } = {}): Promise<void> {
    this.status = 'loading';
    this.missing = false;
    this.pagesReady = false;
    this.#readings += 1;
    const reading = this.#readings;
    const membersTold = this.#people.told;
    const mark = this.#writes.forgettings;
    const initialPageId = options.pageId;
    const speculative = Boolean(initialPageId && options.speculative);
    try {
      this.#stopWatching?.();
      this.#stopWatching = this.workspace.backend.watchContainer(this.wikiId, (change) => {
        this.#apply(change);
      });
      const backend = this.workspace.backend;
      const wikiRead = containerOfWorkspace(backend, this.wikiId, this.workspace.info?.id, 'wiki');
      const membersRead = backend.members(this.wikiId);
      const pagesRead = this.#wikis.pages(this.wikiId);
      pagesRead.catch(() => undefined);
      if (initialPageId) {
        this.speculativePageId = speculative ? initialPageId : undefined;
        this.showPage(initialPageId, speculative);
      } else {
        this.speculativePageId = undefined;
      }
      const [wiki, members] = await Promise.all([wikiRead, membersRead]);
      if (this.#closed || reading !== this.#readings) return;
      void this.workspace.learnUsers(members.map(memberId), { members: true });
      this.takeWiki(wiki);
      if (membersTold === this.#people.told) this.members = members;
      if (initialPageId && (wiki.visibility === 'open' || this.role !== '')) this.status = 'ready';
      const pages = await pagesRead;
      if (this.isClosed() || reading !== this.#readings) return;
      const read = this.#writes.admitted(pages, mark);
      this.pages =
        this.pages.length > 0 ? reread(this.pages, read, [], (p) => this.#writes.receive(p)) : read;
      this.#writes.remember(this.pages);
      this.pagesReady = true;
      this.status = 'ready';
      const remembered = this.workspace.rememberedWikiPage(this.wikiId);
      if (remembered && !this.page(remembered)) {
        this.workspace.forgetWikiPage(this.wikiId, remembered);
      }
      if (this.speculativePageId && !this.page(this.speculativePageId)) {
        this.speculativeFailed(this.speculativePageId);
      } else if (this.openPage?.status === 'ready' && !this.page(this.openPage.pageId)) {
        this.openPage.gone('others', this.openPage.meta?.title ?? '');
      }
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      this.speculativePageId = undefined;
      this.openPage?.close();
      this.openPage = undefined;
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.workspace.evictContainer(this.wikiId);
        this.missing = true;
      } else this.workspace.report('load', error);
      this.status = 'failed';
    }
  }

  speculativeFailed(pageId: string): void {
    this.workspace.forgetWikiPage(this.wikiId, pageId);
    if (this.speculativePageId !== pageId) return;
    this.speculativePageId = undefined;
    if (this.openPage?.pageId === pageId) {
      this.openPage.close();
      this.openPage = undefined;
    }
  }

  join(): Promise<boolean> {
    return joinOpenWiki(this);
  }

  isClosed(): boolean {
    return this.#closed;
  }

  settleBody(): Promise<void> {
    return this.openPage?.settleBody() ?? Promise.resolve();
  }

  close(): void {
    this.#closed = true;
    this.#stopWatching?.();
    this.#stopWatching = undefined;
    this.openPage?.close();
    this.openPage = undefined;
  }

  showPage(pageId: string | undefined, speculative = false): PageStore | undefined {
    if (!speculative) this.speculativePageId = undefined;
    const open = this.openPage;
    if (open && open.pageId === pageId && !open.missing) return open;
    this.openPage?.close();
    this.openPage = pageId === undefined ? undefined : new PageStore(this, pageId, { speculative });
    void this.openPage?.load();
    return this.openPage;
  }

  // ------------------------------------------------------------ changes

  takeWiki(wiki: Container): void {
    if (!this.wiki || this.wiki.updatedAt <= wiki.updatedAt) this.wiki = wiki;
  }

  #apply(change: Change): void {
    switch (change.kind) {
      case 'container':
        if (change.container.id !== this.wikiId) break;
        if (this.missing) void this.load();
        else this.takeWiki(change.container);
        break;
      case 'containerDeleted':
        if (change.containerId !== this.wikiId) break;
        this.workspace.evictContainer(this.wikiId);
        if (this.wiki?.visibility === 'open' && !this.#closed) void this.load();
        else [this.missing, this.status] = [true, 'failed'];
        break;
      case 'pages':
        for (const page of change.pages) this.receivePage(page);
        break;
      case 'blocks':
        for (const block of change.blocks)
          if (isCommentBlock(block)) this.openPage?.comments.receive(block);
        break;
      case 'blocksDeleted': {
        this.#writes.forget(change.blockIds, 'others');
        const remembered = this.workspace.rememberedWikiPage(this.wikiId);
        if (
          remembered &&
          (change.blockIds.includes(remembered) || (this.pagesReady && !this.page(remembered)))
        ) {
          this.workspace.forgetWikiPage(this.wikiId, remembered);
        }
        this.openPage?.comments.forget(change.blockIds);
        break;
      }
      case 'members':
        if (change.containerId !== this.wikiId) break;
        this.#people.told += 1;
        this.members = change.members;
        if (this.workspace.me && !change.members.some((m) => m.userId === this.workspace.me?.id)) {
          this.workspace.evictContainer(this.wikiId);
        }
        void this.workspace.learnUsers(change.members.map(memberId), { members: true });
        break;
      case 'resync':
        void this.#refresh();
        break;
      default:
        break;
    }
  }

  receivePage(page: WikiPage): void {
    if (this.#writes.receive(page)) this.#takePage(page);
  }

  #takePage(page: WikiPage): void {
    if (page.wikiId !== this.wikiId || !this.#writes.admits(page)) return;
    this.pages = take(this.pages, page);
    const shown = this.openPage;
    if (shown?.pageId !== page.id) return;
    if (shown.missing) void shown.load();
    else shown.pageChanged(page);
  }

  async #refresh(): Promise<void> {
    if (this.status !== 'ready' || !this.pagesReady || this.isClosed()) return;
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
      if (this.isClosed() || reading !== this.#readings) return;
      this.takeWiki(wiki);
      if (membersTold === this.#people.told) this.members = members;
      const shown = this.openPage;
      const title = (shown && this.page(shown.pageId)?.title) ?? '';
      const read = this.#writes.admitted(pages, mark);
      this.pages = reread(this.pages, read, before, (page) => this.#writes.receive(page));
      this.#writes.remember(this.pages);
      if (shown && !this.page(shown.pageId)) {
        if (!this.#writes.isDeleting(shown.pageId)) shown.gone('others', title);
      } else if (shown) void shown.refresh();
    } catch (error) {
      if (isBackendError(error) && error.status === 401) {
        this.workspace.report('load', error);
        return;
      }
      if (this.isClosed() || reading !== this.#readings) return;
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.workspace.evictContainer(this.wikiId);
        void this.load();
      }
    }
  }

  // ------------------------------------------------------------- writes

  createPage(draft: PageDraft): Promise<WikiPageDetail | undefined> {
    return this.#writes.create(draft, 'createPage');
  }

  moving = $state(0);
  readonly settling = $derived.by(() => this.moving > 0 || this.workspace.undoBusy);

  async movePage(pageId: string, to: PagePlace): Promise<boolean> {
    this.moving += 1;
    try {
      return await this.#writes.move(pageId, to, 'movePage');
    } finally {
      this.moving -= 1;
    }
  }

  duplicatePage(pageId: string, title: string): Promise<WikiPageDetail | undefined> {
    return this.#writes.duplicate(pageId, title, 'duplicatePage');
  }

  async deletePage(pageId: string): Promise<boolean> {
    const done = await this.#writes.remove(pageId, 'deletePage');
    const remembered = this.workspace.rememberedWikiPage(this.wikiId);
    if (done && remembered && !this.page(remembered)) {
      this.workspace.forgetWikiPage(this.wikiId, remembered);
    }
    return done;
  }

  async deletedPages(paging?: Paging): Promise<DeletedItem[]> {
    await this.#writes.deletionsAnswered();
    return this.#wikis.deletedPages(this.wikiId, paging);
  }

  restorePage(pageId: string): Promise<void> {
    return this.#writes.restore(pageId);
  }

  changePage(pageId: string, looks: PageLooks): Promise<void> {
    return performPageLooks(this, this.#wikis, pageId, looks);
  }

  changeWiki(
    looks: WikiLooks,
    step: 'changeWiki' | 'changeVisibility' | 'changeMinimumRole' = 'changeWiki',
  ): Promise<void> {
    return performWikiLooks(this, looks, step);
  }

  // ------------------------------------------------------------ sharing

  get containerId(): string {
    return this.wikiId;
  }

  get container(): Container | undefined {
    return this.wiki;
  }

  set container(wiki: Container | undefined) {
    this.wiki = wiki;
  }

  setOpen(open: boolean): Promise<void> {
    return this.changeWiki({ visibility: open ? 'open' : 'private' }, 'changeVisibility');
  }

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
