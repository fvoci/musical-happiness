/**
 * The wikis of a workspace and their pages, part of the data boundary
 * (`backend.ts`, `Backend.wiki`).
 *
 * A wiki is a board of the kind `wiki` (`Board.kind`): it is read, changed,
 * deleted, brought back, joined and filed in the sidebar as a board is, by
 * the board's operations. What only a wiki has is here: its pages, a tree
 * (each names the page it is below), each with a title, an icon and a body.
 * The body is written together in the page's room, as a card's is
 * (`Backend.openDoc` with the page in place of the card).
 */
import type { DocNode } from '../editor/schema.ts';
import type { Board, BoardType, MemberRole } from '../logic/wire.ts';

/** A page as the tree of its wiki lists it: without its body. */
export interface WikiPage {
  id: string;
  wikiId: string;
  /** The page it is below; empty at the top of the wiki. */
  parentId: string;
  title: string;
  /** At most one grapheme; empty for none. */
  icon: string;
  /**
   * Its place among the pages of the same parent: they are in the order of
   * these as strings compare by code unit, then of `id` (`logic/page-tree.ts`).
   * The backend makes it; the screens only sort by it.
   */
  order: string;
  createAt: number;
  updateAt: number;
}

/** A page read on its own: with its body and who made and last changed it. */
export interface WikiPageDetail extends WikiPage {
  body: DocNode | null;
  createdBy: string;
  modifiedBy: string;
}

/** What a wiki is made from. Whoever makes it is its admin. */
export interface WikiDraft {
  title: string;
  description?: string;
  icon?: string;
  /** `O`: for whoever works in the workspace to find and join; `P`: for whom it is shared with. */
  type: BoardType;
  minimumRole?: Exclude<MemberRole, '' | 'admin'>;
}

/** A page to make; the backend gives it its identifier, times and place (`order`). */
export interface PageDraft {
  title?: string;
  icon?: string;
  /** The page it goes below, of the same wiki; empty or left out, the top of the wiki. */
  parentId?: string;
  /**
   * The page it goes right after, of the same parent: empty for the first
   * place, left out for the last.
   */
  after?: string;
}

/** What a page is changed to; what is left out stays. Where it is in the tree is not changed here. */
export interface PagePatch {
  title?: string;
  icon?: string;
  /** A whole new body, sent with `readAt`. */
  body?: DocNode | null;
  /** The `updateAt` the page was read at: a page changed since is refused (409) and left as it is. */
  readAt?: number;
}

export interface Wikis {
  /** The wikis of the workspace the reader can see: as the boards of `Backend.boards`, no templates. */
  wikis(workspaceId: string): Promise<Board[]>;
  /** Makes a wiki, filed under the reader's default category; its members are told. */
  createWiki(workspaceId: string, draft: WikiDraft): Promise<Board>;
  /** Every page of the wiki, in no promised order. For its members (an open one: 403 until joined). */
  pages(wikiId: string): Promise<WikiPage[]>;
  page(wikiId: string, pageId: string): Promise<WikiPageDetail>;
  /** For the wiki's editors. A parent or `after` that is no such page is 400. */
  createPage(wikiId: string, draft: PageDraft): Promise<WikiPageDetail>;
  patchPage(wikiId: string, pageId: string, patch: PagePatch): Promise<WikiPageDetail>;
  /**
   * Moves the page and every page below it to the trash, for the wiki's
   * editors. The wiki's listeners are told of the page alone (`blocksDeleted`).
   */
  deletePage(wikiId: string, pageId: string): Promise<void>;
}

/** Where a backend keeps its wikis. One by a public link has none (W4). */
export interface WikisOfBackend {
  readonly wiki?: Wikis;
}
