/**
 * The next cards that completing cards that repeat makes (`logic/recurrence.ts`):
 * which next card a change made, how its undo takes that card back (the
 * completed card repeats again and its next card goes), and the notice that
 * tells the one who made it, with a way to undo it.
 */
import type { Recurrence } from '../logic/recurrence.ts';
import { cardRecurredTo, cardRecurrence, type Card } from '../logic/wire.ts';

import { deleteCardForUndo } from './board-sync.ts';
import type { BoardStore } from './board.svelte.ts';
import type { Workspace } from './workspace.svelte.ts';

/** A next card a change made: what the undo of the change takes back. */
export interface MadeNext {
  nextId: string;
  /** How the completed card repeated: its undo hands it back. */
  recurrence: Recurrence;
}

/** The next card that `after` names and `before`, which repeated, did not: the change made it. */
export function nextMadeBy(
  before: Card | undefined,
  after: Card | undefined,
): MadeNext | undefined {
  if (!before || !after) return undefined;
  const nextId = cardRecurredTo(after);
  const recurrence = cardRecurrence(before);
  if (!nextId || !recurrence || nextId === cardRecurredTo(before)) return undefined;
  return { nextId, recurrence };
}

/** Takes back a next card, in the undo of the change that made it; one gone already is gone. */
export async function takeBackNext(store: BoardStore, nextId: string): Promise<void> {
  await deleteCardForUndo(store, nextId);
  store.cards = store.cards.filter((card) => card.id !== nextId);
}

export interface NextCardNotice {
  /** Tells one notice from the next. */
  id: number;
  /** The number of the next card on its board; unknown when it could not be read. */
  number: number | undefined;
  /** How many next cards the change made: more than one when several cards were moved at once. */
  count: number;
  /** The step of the change in the history of undo (`UndoManager.currentCheckpoint`). */
  checkpoint: number;
}

/** How long a notice is shown. */
export const NOTICE_MS = 8000;

let noticeCounter = 0;

/** The notice of the next cards a person's change made, one at a time, for a workspace; it goes after `NOTICE_MS`. */
export class NextCardNotices {
  notice = $state.raw<NextCardNotice | undefined>(undefined);
  readonly #workspace: Workspace;

  constructor(workspace: Workspace) {
    this.#workspace = workspace;
  }

  /** Says that the change at `checkpoint` made `made` on the board of `store`. */
  async say(store: BoardStore, made: readonly MadeNext[], checkpoint: number): Promise<void> {
    const first = made[0];
    if (!first) return;
    let number: number | undefined;
    try {
      const { body: _body, ...card } = await store.workspace.backend.card(
        store.boardId,
        first.nextId,
      );
      store.receiveCard(card);
      number = card.number;
    } catch {
      // Gone already, or not to be read: said without its number.
    }
    noticeCounter += 1;
    const id = noticeCounter;
    this.notice = { id, number, count: made.length, checkpoint };
    setTimeout(() => {
      this.dismiss(id);
    }, NOTICE_MS);
  }

  dismiss(id: number): void {
    if (this.notice?.id === id) this.notice = undefined;
  }

  /**
   * Undoes the change the notice is of, while it is the change to undo; after
   * a later one it only goes (that change is undone first, as undo always does).
   */
  async undo(): Promise<void> {
    const notice = this.notice;
    if (!notice) return;
    this.notice = undefined;
    const undo = this.#workspace.undo;
    if (undo.canUndo && undo.currentCheckpoint === notice.checkpoint) {
      await this.#workspace.undoLast();
    }
  }
}

const notices = new WeakMap<Workspace, NextCardNotices>();

/** The notices of next cards of a workspace. */
export function nextCardNotices(workspace: Workspace): NextCardNotices {
  let found = notices.get(workspace);
  if (!found) {
    found = new NextCardNotices(workspace);
    notices.set(workspace, found);
  }
  return found;
}
