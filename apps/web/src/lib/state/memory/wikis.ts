/**
 * The wikis of the backend in memory (`Backend.wiki`), as the server keeps
 * them: a wiki is a board of the kind `wiki` (so the board's operations read,
 * change, delete and bring it back), its pages are blocks of the type `page`
 * with their icon and place among their `fields`.
 *
 * The rules are the server's (`crates/store/src/pages.rs`): a page goes below
 * a page of the same wiki or at its top, among its siblings where `after`
 * says (`orderBetween`); its pages are for the wiki's members, written by its
 * editors; a body is written with the `updateAt` it was read at (409 when the
 * page changed since, what its room took counted first); deleting a page
 * takes every page below it along, and tells of the page alone.
 */
import type { DocNode } from '../../editor/schema.ts';
import { createBoard, type Board, type BoardKind } from '../../logic/wire.ts';
import { BackendError } from '../backend.ts';
import type {
  PageDraft,
  PagePatch,
  WikiDraft,
  WikiPage,
  WikiPageDetail,
  Wikis,
} from '../wiki-types.ts';

import { memberRole } from './board-access.ts';
import { checkedWhole } from './bodies.ts';
import { orderBetween } from './page-order.ts';
import { fail } from './refuse.ts';
import { copy, memberOf, orderOf, pageOf, scramble } from './shapes.ts';
import { deletion } from './trash.ts';
import type { StoredBlock, World } from './world.ts';

/** What the wikis share with the backend they are of. */
export interface WikiHost {
  readonly world: World;
  readonly userId: string;
  /** Milliseconds every call waits before it answers. */
  readonly latency: number;
}

/** The most characters of a page's title, as the server takes it. */
export const PAGE_TITLE_MAX = 255;

const isWiki = (board: Board | undefined): board is Board => board?.kind === 'wiki';

/** The order of a page among its siblings: by `order` as code units compare, then by `id`. */
const byPlace = (first: StoredBlock, second: StoredBlock): number => {
  const [one, other] = [orderOf(first), orderOf(second)];
  if (one !== other) return one < other ? -1 : 1;
  if (first.id === second.id) return 0;
  return first.id < second.id ? -1 : 1;
};

function detailOf(block: StoredBlock): WikiPageDetail {
  return {
    ...pageOf(block),
    body: copy(block.body ?? null),
    createdBy: block.createdBy,
    modifiedBy: block.modifiedBy,
  };
}

/** One grapheme at most, as the server takes an icon. */
function checkedIcon(icon: string): string {
  const graphemes = [...new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment(icon)];
  if (graphemes.length > 1) fail(400, 'an icon is one grapheme', { field: 'icon' });
  return icon;
}

function checkedTitle(title: string): string {
  // In characters (code points), as the server counts them.
  if (Array.from(title).length > PAGE_TITLE_MAX) {
    fail(400, 'the title is too long', { field: 'title', limit: PAGE_TITLE_MAX });
  }
  return title;
}

export class MemoryWikis implements Wikis {
  readonly #host: WikiHost;
  readonly #failures = new Map<keyof Wikis, BackendError>();

  constructor(host: WikiHost) {
    this.#host = host;
  }

  get #world(): World {
    return this.#host.world;
  }

  /** Makes the next call of `method` reject with `error`, once. */
  failNext(method: keyof Wikis, error: BackendError): void {
    this.#failures.set(method, error);
  }

  async #enter(method: keyof Wikis): Promise<void> {
    if (this.#host.latency > 0) {
      await new Promise((resolve) => {
        setTimeout(resolve, this.#host.latency);
      });
    } else {
      await Promise.resolve();
    }
    const failure = this.#failures.get(method);
    if (failure) {
      this.#failures.delete(method);
      throw failure;
    }
  }

  #role(wikiId: string, kind: BoardKind = 'wiki'): string {
    return memberRole(this.#world, this.#host.userId, wikiId, kind);
  }

  /** For the wiki's editors: its admins and editors. */
  #mayWrite(wikiId: string): void {
    const role = this.#role(wikiId);
    if (role !== 'admin' && role !== 'editor') fail(403, 'access denied to the wiki');
  }

  #page(wikiId: string, pageId: string): StoredBlock {
    const block = this.#world.blocks.get(pageId);
    if (block?.type !== 'page' || block.boardId !== wikiId) fail(404, 'no such page');
    return block;
  }

  #pagesOf(wikiId: string): StoredBlock[] {
    return this.#world.blocksOf(wikiId).filter((block) => block.type === 'page');
  }

  #tellPages(wikiId: string, blocks: readonly StoredBlock[]): void {
    this.#world.tellBoard(wikiId, { kind: 'pages', boardId: wikiId, pages: blocks.map(pageOf) });
  }

  /** As the server: the reader's wikis and the workspace's open ones, oldest first. */
  async wikis(workspaceId: string): Promise<Board[]> {
    await this.#enter('wikis');
    if (workspaceId !== this.#world.workspace.id) fail(404, 'no such workspace');
    const userId = this.#host.userId;
    const theirs = [...this.#world.boards.values()].filter(
      (board) =>
        isWiki(board) &&
        board.workspaceId === workspaceId &&
        (board.type === 'O' ||
          (this.#world.members.get(board.id) ?? []).some((member) => member.userId === userId)),
    );
    return scramble(theirs).map(copy);
  }

  async createWiki(workspaceId: string, draft: WikiDraft): Promise<Board> {
    await this.#enter('createWiki');
    if (workspaceId !== this.#world.workspace.id) fail(404, 'no such workspace');
    const world = this.#world;
    const userId = this.#host.userId;
    const now = world.now();
    const wiki: Board = {
      ...createBoard(),
      id: world.newId(),
      workspaceId,
      kind: 'wiki',
      createdBy: userId,
      modifiedBy: userId,
      type: draft.type,
      minimumRole: draft.minimumRole ?? 'editor',
      title: draft.title,
      description: draft.description ?? '',
      icon: draft.icon ?? '',
      cardProperties: [],
      createAt: now,
      updateAt: now,
    };
    world.boards.set(wiki.id, wiki);
    world.members.set(wiki.id, [memberOf(wiki.id, userId, 'admin')]);
    // Filed under the maker's default category, as a board is.
    const fallback = world.categories.get(userId)?.find((category) => category.type === 'system');
    fallback?.boardIds.push(wiki.id);
    world.tellBoard(wiki.id, { kind: 'board', board: copy(wiki) });
    world.tellUsers([userId], { kind: 'board', board: copy(wiki) });
    if (fallback)
      world.tellUsers([userId], {
        kind: 'categories',
        categories: copy(world.categories.get(userId) ?? []),
      });
    return copy(wiki);
  }

  async pages(wikiId: string): Promise<WikiPage[]> {
    await this.#enter('pages');
    this.#role(wikiId);
    return scramble(this.#pagesOf(wikiId)).map(pageOf);
  }

  async page(wikiId: string, pageId: string): Promise<WikiPageDetail> {
    await this.#enter('page');
    this.#role(wikiId);
    return detailOf(this.#page(wikiId, pageId));
  }

  /** The place of a new page below `parentId`, as `after` asks (`PageDraft.after`). */
  #placeFor(wikiId: string, parentId: string, after: string | undefined): string {
    const siblings = this.#pagesOf(wikiId)
      .filter((block) => block.parentId === parentId)
      .sort(byPlace);
    const orderAt = (at: number): string | undefined => {
      const sibling = siblings[at];
      return sibling && orderOf(sibling);
    };
    if (after === undefined) return orderBetween(orderAt(siblings.length - 1), undefined);
    if (after === '') return orderBetween(undefined, orderAt(0));
    const at = siblings.findIndex((block) => block.id === after);
    if (at < 0) fail(400, 'no page of the same parent', { field: 'after' });
    return orderBetween(orderAt(at), orderAt(at + 1));
  }

  async createPage(wikiId: string, draft: PageDraft): Promise<WikiPageDetail> {
    await this.#enter('createPage');
    this.#mayWrite(wikiId);
    const parentId = draft.parentId === wikiId ? '' : (draft.parentId ?? '');
    if (parentId !== '') {
      const parent = this.#world.blocks.get(parentId);
      if (parent?.type !== 'page' || parent.boardId !== wikiId) {
        fail(400, 'the parent is no page of the wiki', { field: 'parentId' });
      }
    }
    const order = this.#placeFor(wikiId, parentId, draft.after);
    const now = this.#world.now();
    const userId = this.#host.userId;
    const page: StoredBlock = {
      id: this.#world.newId(),
      boardId: wikiId,
      parentId,
      createdBy: userId,
      modifiedBy: userId,
      schema: 1,
      type: 'page',
      title: checkedTitle(draft.title ?? ''),
      fields: { icon: checkedIcon(draft.icon ?? ''), order },
      body: null,
      createAt: now,
      updateAt: now,
      deleteAt: 0,
    };
    this.#world.blocks.set(page.id, page);
    this.#tellPages(wikiId, [page]);
    return detailOf(page);
  }

  async patchPage(wikiId: string, pageId: string, patch: PagePatch): Promise<WikiPageDetail> {
    await this.#enter('patchPage');
    this.#mayWrite(wikiId);
    const page = this.#page(wikiId, pageId);
    if (patch.body !== undefined && patch.readAt === undefined) {
      fail(400, 'a body is written with the time it was read', { field: 'readAt' });
    }
    // What its room took is written into the page first: it counts against `readAt`.
    const body: DocNode | null | undefined =
      patch.body === undefined ? undefined : checkedWhole(this.#world, wikiId, pageId, patch.body);
    if (patch.readAt !== undefined && patch.readAt !== page.updateAt) {
      throw new BackendError(409, { detail: 'the page changed since it was read' });
    }
    if (patch.title !== undefined) page.title = checkedTitle(patch.title);
    if (patch.icon !== undefined) page.fields = { ...page.fields, icon: checkedIcon(patch.icon) };
    if (body !== undefined) page.body = body;
    page.updateAt = this.#world.now();
    page.modifiedBy = this.#host.userId;
    if (body !== undefined) this.#world.bodyWritten?.(pageId);
    this.#tellPages(wikiId, [page]);
    return detailOf(page);
  }

  async deletePage(wikiId: string, pageId: string): Promise<void> {
    await this.#enter('deletePage');
    this.#mayWrite(wikiId);
    const page = this.#page(wikiId, pageId);
    const gone = [page];
    // Each page found brings the pages below it: the list grows as it is walked.
    for (const above of gone) {
      gone.push(...this.#pagesOf(wikiId).filter((block) => block.parentId === above.id));
    }
    this.#world.deletedBlocks.set(pageId, {
      boardId: wikiId,
      blocks: gone,
      ...deletion({ world: this.#world, userId: this.#host.userId }),
    });
    for (const block of gone) this.#world.blocks.delete(block.id);
    this.#world.tellBoard(wikiId, { kind: 'blocksDeleted', boardId: wikiId, blockIds: [pageId] });
  }
}
