/**
 * What bodies name, for the backend in memory, as the server answers it
 * (`resolveReferences`, `getReferencesTo`, `searchReferences`): a card is told
 * to whoever has a role on its board, a page to whoever reads its wiki (a role
 * on it, or work in the workspace of an open one), and neither to anybody else,
 * whatever is asked.
 */
import type { DocNode } from '../../editor/schema.ts';
import { REFERENCES_AT_ONCE, type LinkedBlock, type References } from '../references.ts';
import type { Paging } from '../trash-reads.ts';

import { reads } from './board-access.ts';
import { fail } from './refuse.ts';
import type { StoredBlock, World } from './world.ts';

/** The most cards a search finds (the server's). */
const FOUND_AT_MOST = 20;

/** The identifiers the references of a body hold, in the order they stand. */
export function namedIn(node: DocNode | null | undefined): string[] {
  if (!node) return [];
  const own =
    node.type === 'reference' && typeof node.attrs?.id === 'string' ? [node.attrs.id] : [];
  return [...own, ...(node.content ?? []).flatMap(namedIn)];
}

/** The body with what its references name renamed, where `rename` has another name. */
export function renameNamed(node: DocNode, rename: (id: string) => string): DocNode {
  const attrs =
    node.type === 'reference' && typeof node.attrs?.id === 'string'
      ? { attrs: { ...node.attrs, id: rename(node.attrs.id) } }
      : {};
  const content = node.content
    ? { content: node.content.map((each) => renameNamed(each, rename)) }
    : {};
  return { ...node, ...attrs, ...content };
}

/** Who asks, and of which world. */
interface Asker {
  readonly world: World;
  readonly userId: string;
}

export class MemoryReferences implements References {
  readonly #asker: Asker;
  readonly #enter: () => Promise<void>;

  constructor(asker: Asker, enter: () => Promise<void>) {
    this.#asker = asker;
    this.#enter = enter;
  }

  /** The card or the page as a reference shows it, if the reader is on its board or its wiki. */
  #shown(block: StoredBlock | undefined): LinkedBlock | undefined {
    const { world, userId } = this.#asker;
    if (block?.type !== 'card' && block?.type !== 'page') return undefined;
    if (!reads(world, userId, block.boardId)) return undefined;
    const icon = typeof block.fields.icon === 'string' ? block.fields.icon : '';
    const { id, title, boardId } = block;
    return block.type === 'page'
      ? { id, kind: 'page', title, icon, boardId, boardKind: 'wiki' }
      : { id, kind: 'card', title, icon, boardId, boardKind: 'board' };
  }

  #check(workspaceId: string): void {
    if (workspaceId !== this.#asker.world.workspace.id) fail(404, 'no such workspace');
  }

  async resolve(workspaceId: string, ids: readonly string[]): Promise<LinkedBlock[]> {
    await this.#enter();
    this.#check(workspaceId);
    if (ids.length > REFERENCES_AT_ONCE) {
      fail(400, `at most ${String(REFERENCES_AT_ONCE)} references are resolved at once`);
    }
    const blocks = this.#asker.world.blocks;
    return [...new Set(ids)].flatMap((id) => this.#shown(blocks.get(id)) ?? []);
  }

  async to(workspaceId: string, blockId: string, paging?: Paging): Promise<LinkedBlock[]> {
    await this.#enter();
    this.#check(workspaceId);
    const blocks = this.#asker.world.blocks;
    if (!this.#shown(blocks.get(blockId))) fail(404, 'no such block');
    const naming = new Map<string, StoredBlock>();
    for (const block of blocks.values()) {
      if (!namedIn(block.body).includes(blockId)) continue;
      // What a comment names, the card or the page it is on names.
      const source = block.type === 'comment' ? blocks.get(block.parentId) : block;
      if (source && source.id !== blockId) naming.set(source.id, source);
    }
    const shown = [...naming.values()]
      .sort((a, b) => a.createAt - b.createAt || a.id.localeCompare(b.id))
      .flatMap((block) => this.#shown(block) ?? []);
    if (!paging || paging.perPage <= 0) return shown;
    return shown.slice(paging.page * paging.perPage, (paging.page + 1) * paging.perPage);
  }

  async search(workspaceId: string, term: string): Promise<LinkedBlock[]> {
    await this.#enter();
    this.#check(workspaceId);
    const words = term
      .toLowerCase()
      .split(/\s+/)
      .filter((word) => word !== '');
    const { world } = this.#asker;
    return [...world.blocks.values()]
      .filter((block) => block.fields.isTemplate !== true)
      .filter((block) => world.boards.get(block.boardId)?.isTemplate !== true)
      .filter((block) => words.every((word) => block.title.toLowerCase().includes(word)))
      .sort((a, b) => b.updateAt - a.updateAt || a.id.localeCompare(b.id))
      .flatMap((block) => this.#shown(block) ?? [])
      .slice(0, FOUND_AT_MOST);
  }
}
