/**
 * What the references of the bodies on screen name, for the reader: asked of
 * the backend once (`References.resolve`, all that is drawn in one turn in
 * one request), then held and kept current by the change messages of the
 * boards the named cards are on. A card that is renamed shows its new title,
 * one that is deleted shows as what the reader may not see. When the
 * connection was away everything held is asked again, and when a board comes
 * into the reader's sight, what they could not see is.
 *
 * One of these belongs to a `Workspace` (`referenceTitles`); what it holds is
 * kept by the workspace entered, where an identifier is asked.
 */
import { SvelteMap } from 'svelte/reactivity';

import { cardIcon } from '../logic/wire.ts';

import { isBackendError, isCardBlock, type Change } from './backend.ts';
import type { BoardStore } from './board.svelte.ts';
import { REFERENCES_AT_ONCE, type LinkedBlock } from './references.ts';
import type { Paging } from './trash-reads.ts';
import type { Workspace } from './workspace.svelte.ts';

/** Where a reference leads. The app hands it in: it has the address bar. */
export interface ReferencePlaces {
  /** The address of what is named; none where the app has no screen for it. */
  href(linked: LinkedBlock): string | undefined;
  /** Shows what is named. */
  open(linked: LinkedBlock): void;
  /** The identifier of the card or the page an address of this workspace names. */
  named(address: string): string | undefined;
}

/** How long a search waits for the next letter before it asks. */
const SEARCH_WAIT_MS = 150;
/** How many cards of the open board are offered before anything is typed. */
const OFFERED_UNASKED = 8;

/** The identifiers asked together in one turn, and the answer they wait for. */
interface Batch {
  workspaceId: string;
  ids: string[];
  answered: Promise<void>;
}

export class ReferenceTitles {
  readonly #workspace: Workspace;
  /** Where references lead, once the app has said. */
  places = $state.raw<ReferencePlaces>();
  /**
   * What is known, by workspace and identifier: `null` for what the reader may
   * not see. Whoever reads an entry where state is followed reads it again
   * when it changes.
   */
  readonly #known = new SvelteMap<string, LinkedBlock | null>();
  /** What is being asked. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- nothing that is drawn reads it
  readonly #pending = new Map<string, Promise<void>>();
  #batch: Batch | undefined;
  /** The boards whose changes are followed, each with what stops that. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- nothing that is drawn reads it
  readonly #watched = new Map<string, () => void>();
  #following = false;
  #searches = 0;
  #latest: Promise<LinkedBlock[]> = Promise.resolve([]);

  constructor(workspace: Workspace) {
    this.#workspace = workspace;
  }

  /** The workspace an identifier is asked in; a reader by a link has none. */
  #here(): string {
    return this.#workspace.info?.id ?? '';
  }

  /**
   * What a reference shows: `undefined` while it is being asked (it is asked
   * by this), `null` for what the reader may not see. Read in an effect, it
   * runs again when the answer comes and whenever what it shows changes.
   */
  of(id: string): LinkedBlock | null | undefined {
    const key = `${this.#here()}/${id}`;
    if (this.#known.has(key)) return this.#known.get(key);
    void this.#ask([id]);
    return undefined;
  }

  /** Whether the reader may see what the identifier names, asked if it is not known. */
  async seen(id: string): Promise<boolean> {
    const key = `${this.#here()}/${id}`;
    if (!this.#known.has(key)) await this.#ask([id]);
    return (this.#known.get(key) ?? null) !== null;
  }

  /** What names the block, a page of it; what is read is known from then on. */
  async to(blockId: string, paging?: Paging): Promise<LinkedBlock[]> {
    const workspaceId = this.#here();
    const found = await this.#workspace.backend.references.to(workspaceId, blockId, paging);
    this.#learn(workspaceId, found);
    return found;
  }

  /**
   * What may be named whose title has the words of `term`. Asked once the
   * typing rests, and only the last term typed is: an earlier one is answered
   * as the last. Before anything is typed, the cards of `board` changed last.
   * What is offered is known from then on: taken, it is drawn without asking.
   */
  search(term: string, board?: BoardStore): Promise<LinkedBlock[]> {
    this.#searches += 1;
    const mine = this.#searches;
    const workspaceId = this.#here();
    const found = (async (): Promise<LinkedBlock[]> => {
      if (term.trim() === '') {
        const recent = board ? recentOf(board) : [];
        this.#learn(workspaceId, recent);
        return recent;
      }
      await new Promise((resolve) => setTimeout(resolve, SEARCH_WAIT_MS));
      if (mine !== this.#searches) return this.#latest;
      try {
        const blocks = await this.#workspace.backend.references.search(workspaceId, term);
        this.#learn(workspaceId, blocks);
        return mine === this.#searches ? blocks : await this.#latest;
      } catch (error) {
        this.#failed(error);
        return [];
      }
    })();
    this.#latest = found;
    return found;
  }

  #failed(error: unknown): void {
    // A session that is over signs out, whichever workspace is shown by then.
    if (isBackendError(error) && error.status === 401) this.#workspace.report('load', error);
    else if (!isBackendError(error)) console.error(error);
  }

  #learn(workspaceId: string, blocks: readonly LinkedBlock[]): void {
    for (const linked of blocks) this.#set(workspaceId, linked.id, linked);
  }

  #set(workspaceId: string, id: string, linked: LinkedBlock | null): void {
    this.#known.set(`${workspaceId}/${id}`, linked);
    if (!linked || this.#watched.has(linked.boardId)) return;
    const stop = this.#workspace.backend.watchBoard(linked.boardId, (change) => {
      this.#heard(change);
    });
    this.#watched.set(linked.boardId, stop);
  }

  /** Asks for the identifiers that are not being asked, with all that is asked this turn. */
  #ask(ids: readonly string[]): Promise<void> {
    const workspaceId = this.#here();
    this.#follow();
    const waited: Promise<void>[] = [];
    for (const id of ids) {
      const key = `${workspaceId}/${id}`;
      const asked = this.#pending.get(key);
      if (asked) {
        waited.push(asked);
        continue;
      }
      let batch = this.#batch;
      if (batch?.workspaceId !== workspaceId) {
        const made: Batch = { workspaceId, ids: [], answered: Promise.resolve() };
        made.answered = Promise.resolve().then(() => {
          if (this.#batch === made) this.#batch = undefined;
          return this.#read(made.workspaceId, made.ids);
        });
        this.#batch = batch = made;
      }
      batch.ids.push(id);
      this.#pending.set(key, batch.answered);
      waited.push(batch.answered);
    }
    return Promise.all(waited).then(() => undefined);
  }

  async #read(workspaceId: string, ids: readonly string[]): Promise<void> {
    try {
      for (let start = 0; start < ids.length; start += REFERENCES_AT_ONCE) {
        const part = ids.slice(start, start + REFERENCES_AT_ONCE);
        const found = await this.#workspace.backend.references.resolve(workspaceId, part);
        for (const id of part) {
          this.#set(workspaceId, id, found.find((linked) => linked.id === id) ?? null);
        }
      }
    } catch (error) {
      // What was not answered stays asked for: the next drawing asks again.
      this.#failed(error);
    } finally {
      for (const id of ids) this.#pending.delete(`${workspaceId}/${id}`);
    }
  }

  /** The identifiers known in the workspace entered that `wanted` picks. */
  #knownHere(wanted: (linked: LinkedBlock | null) => boolean): string[] {
    const prefix = `${this.#here()}/`;
    return [...this.#known]
      .filter(([key, linked]) => key.startsWith(prefix) && wanted(linked))
      .map(([key]) => key.slice(prefix.length));
  }

  /** Follows what is the reader's own: a connection that is back, a board that came into sight. */
  #follow(): void {
    if (this.#following) return;
    this.#following = true;
    this.#workspace.backend.watchSelf((change) => {
      if (change.kind === 'resync') void this.#ask(this.#knownHere(() => true));
      else if (change.kind === 'board') void this.#ask(this.#knownHere((linked) => !linked));
    });
  }

  /** A change of a board that something known is on. */
  #heard(change: Change): void {
    const workspaceId = this.#here();
    if (change.kind === 'blocks') {
      for (const block of change.blocks) {
        const key = `${workspaceId}/${block.id}`;
        if (!this.#known.has(key) || !isCardBlock(block)) continue;
        const { id, title, boardId } = block;
        const icon = cardIcon(block);
        this.#known.set(key, { id, kind: 'card', title, icon, boardId, boardKind: 'board' });
      }
    } else if (change.kind === 'blocksDeleted') {
      for (const id of change.blockIds) this.#unsee(`${workspaceId}/${id}`);
    } else if (change.kind === 'boardDeleted') {
      for (const id of this.#knownHere((linked) => linked?.boardId === change.boardId)) {
        this.#unsee(`${workspaceId}/${id}`);
      }
    }
  }

  #unsee(key: string): void {
    if (!this.#known.get(key)) return;
    this.#known.set(key, null);
  }
}

/** The cards of the board that were changed last, as references show them; templates left out. */
function recentOf(board: BoardStore): LinkedBlock[] {
  return [...board.realCards]
    .sort((a, b) => b.updateAt - a.updateAt || a.id.localeCompare(b.id))
    .slice(0, OFFERED_UNASKED)
    .map((card) => ({
      id: card.id,
      kind: 'card',
      title: card.title,
      icon: cardIcon(card),
      boardId: card.boardId,
      boardKind: 'board',
    }));
}

const titles = new WeakMap<Workspace, ReferenceTitles>();

/** What the references shown in a workspace name: one for all its screens. */
export function referenceTitles(workspace: Workspace): ReferenceTitles {
  const held = titles.get(workspace);
  if (held) return held;
  const made = new ReferenceTitles(workspace);
  titles.set(workspace, made);
  return made;
}
