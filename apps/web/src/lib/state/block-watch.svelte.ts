/**
 * Whether the reader watches a card or a page (`Backend.watches`), and the
 * switch of it: watching, they are told when someone comments on it or
 * changes it. Nothing is watched in the reader's own space, where nobody else
 * writes, nor through a public link (no `watches`).
 *
 * The switch shows at once, and goes back with a report when the server
 * refuses; ending a watch that is gone already (404) leaves it ended.
 */
import { isBackendError } from './backend.ts';
import type { WatchedKind } from './watch-types.ts';
import { isOwnSpace } from './workspace-types.ts';
import type { Workspace } from './workspace.svelte.ts';

export class BlockWatch {
  /** Undefined until it is read, and where it cannot be. */
  watching = $state<boolean | undefined>(undefined);
  busy = $state(false);
  readonly #workspace: Workspace;
  readonly #blockId: string;
  readonly #kind: WatchedKind;
  /** A switch made after a reading began wins over what the reading says. */
  #turn = 0;
  #closed = false;

  constructor(workspace: Workspace, blockId: string, kind: WatchedKind) {
    this.#workspace = workspace;
    this.#blockId = blockId;
    this.#kind = kind;
  }

  get canWatch(): boolean {
    const { info, me, backend } = this.#workspace;
    return Boolean(backend.watches && info?.id && me?.id && !isOwnSpace(info));
  }

  async read(): Promise<void> {
    const watches = this.#workspace.backend.watches;
    const workspaceId = this.#workspace.info?.id;
    const meId = this.#workspace.me?.id;
    if (!this.canWatch || !watches || !workspaceId || !meId) return;
    const turn = ++this.#turn;
    try {
      const ids = await watches.watching(workspaceId, meId);
      if (!this.#closed && turn === this.#turn) this.watching = ids.includes(this.#blockId);
    } catch {
      // Not read: the switch is not shown, as where it cannot be.
    }
  }

  async set(next: boolean): Promise<void> {
    const watches = this.#workspace.backend.watches;
    const workspaceId = this.#workspace.info?.id;
    const meId = this.#workspace.me?.id;
    if (!this.canWatch || this.busy || !watches || !workspaceId || !meId) return;
    const before = this.watching;
    this.#turn += 1;
    this.watching = next;
    this.busy = true;
    try {
      if (next) await watches.watch(workspaceId, this.#blockId, this.#kind, meId);
      else await watches.unwatch(workspaceId, this.#blockId, meId);
    } catch (error) {
      if (this.#closed) return;
      if (next || !isBackendError(error) || error.status !== 404) {
        this.watching = before;
        this.#workspace.report('save', error);
      }
    } finally {
      this.busy = false;
    }
  }

  close(): void {
    this.#closed = true;
  }
}
