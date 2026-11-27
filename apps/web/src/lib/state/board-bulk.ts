/**
 * One value of one property put on many cards of a board at once, as the
 * bar of a selection does it (features/views/BulkBar.svelte): one request
 * (`batchPatchCards`), which the server stores all or none, and one step of
 * the history. A function of the board store, which it is given.
 *
 * The cards show the value at once. When the request is refused nothing was
 * stored, and every card that still shows what it wrote is shown again as it
 * was; a card that something else changed meanwhile keeps that change. An
 * answer that was lost (status 0) may have been stored all the same: the
 * cards are read again, and when they hold what was written, that reading
 * is the answer.
 *
 * Undo gives each card back its own value. The route sets one value on all
 * the cards it names, so undo sends one request for each value the cards had
 * (no value at all is one of them); each is a value set, not a change, and
 * may be sent again. Undo and redo leave out the cards that are gone by then.
 *
 * These writes do not go through the store's writes of single cards: a
 * version of a card that arrives while one is under way is shown, and the
 * answer, newer, then takes its place.
 */
import type { PropertyValue } from '../logic/wire.ts';

import type { CardPropertiesPatch } from './authority-types.ts';
import { isBackendError, type CardSummary } from './backend.ts';
import { withCardProperty } from './board-parts.ts';
import { performBoardAction } from './board-sync.ts';
import type { BoardStore } from './board.svelte.ts';
import { sameValue } from './inverse-values.ts';
import { applyCardPatch, cardPatches, isEmptyCardPatch } from './patches.ts';

/** A card's part of a batch: the values to show and send, and those to show again if it fails. */
interface CardWrite {
  cardId: string;
  patch: CardPropertiesPatch;
  restore: CardPropertiesPatch;
}

/** The values a patch sets on a card, by property; a cleared one is undefined. */
function valuesOf(patch: CardPropertiesPatch): Map<string, PropertyValue | undefined> {
  const values = new Map<string, PropertyValue | undefined>(
    Object.entries(patch.updatedProperties ?? {}),
  );
  for (const id of patch.deletedProperties ?? []) values.set(id, undefined);
  return values;
}

/** Puts the changed cards on screen in one assignment. */
function show(store: BoardStore, changed: ReadonlyMap<string, CardSummary>): void {
  if (changed.size === 0) return;
  store.cards = store.cards.map((card) => changed.get(card.id) ?? card);
}

/** The cards that still show what `writes` put on them, shown again as before. */
function takeBack(store: BoardStore, writes: readonly CardWrite[]): void {
  const changed = new Map<string, CardSummary>();
  for (const write of writes) {
    const shown = store.card(write.cardId);
    if (!shown) continue;
    const own: CardPropertiesPatch = {};
    const restore = valuesOf(write.restore);
    for (const [id, value] of valuesOf(write.patch)) {
      if (!sameValue(shown.fields.properties[id], value)) continue;
      const before = restore.get(id);
      if (before === undefined) (own.deletedProperties ??= []).push(id);
      else (own.updatedProperties ??= {})[id] = before;
    }
    if (!isEmptyCardPatch(own)) changed.set(shown.id, applyCardPatch(shown, own));
  }
  show(store, changed);
}

/** Whether every card read holds the values that were written on it. */
function holds(read: readonly CardSummary[], writes: readonly CardWrite[]): boolean {
  return writes.every((write) => {
    const card = read.find((one) => one.id === write.cardId);
    if (!card) return false;
    return [...valuesOf(write.patch)].every(([id, value]) =>
      sameValue(card.fields.properties[id], value),
    );
  });
}

/** Sends one batch: the answer, or what the board holds when the answer was lost and it holds the write. */
async function send(
  store: BoardStore,
  writes: readonly CardWrite[],
  quiet: boolean,
): Promise<CardSummary[]> {
  const { backend } = store.workspace;
  const patch = writes[0]?.patch ?? {};
  const ids = writes.map((write) => write.cardId);
  try {
    return await backend.batchPatchCards(store.boardId, ids, patch, quiet ? { quiet } : undefined);
  } catch (error) {
    if (!isBackendError(error) || error.status !== 0) throw error;
    const read = await backend.cards(store.boardId).catch(() => undefined);
    if (!read || !holds(read, writes)) throw error;
    return read.filter((card) => ids.includes(card.id));
  }
}

/** The writes grouped by the patch they send: one request each. */
function batchesOf(writes: readonly CardWrite[]): CardWrite[][] {
  const batches: CardWrite[][] = [];
  for (const write of writes) {
    const batch = batches.find((one) => sameValue(one[0]?.patch, write.patch));
    if (batch) batch.push(write);
    else batches.push([write]);
  }
  return batches;
}

/**
 * Shows the writes at once and sends them, a batch for each patch; a batch
 * refused is taken back on its cards, and its error is thrown once all
 * have answered.
 */
async function writeCards(
  store: BoardStore,
  writes: readonly CardWrite[],
  quiet: boolean,
): Promise<void> {
  const present = writes.filter((write) => store.card(write.cardId) !== undefined);
  const shown = new Map<string, CardSummary>();
  for (const write of present) {
    const card = store.card(write.cardId);
    if (card) shown.set(card.id, applyCardPatch(card, write.patch));
  }
  show(store, shown);
  const batches = batchesOf(present);
  const answers = await Promise.allSettled(batches.map((batch) => send(store, batch, quiet)));
  answers.forEach((answer, at) => {
    if (answer.status === 'fulfilled') {
      for (const card of answer.value) store.receiveCard(card);
    } else {
      takeBack(store, batches[at] ?? []);
    }
  });
  const refused = answers.find((answer) => answer.status === 'rejected');
  // What a backend throws is a `BackendError`, or the error of a failed reading.
  if (refused) throw refused.reason as Error;
}

/**
 * Sets `propertyId` to `value` on the cards of `cardIds` (an empty value
 * clears it), as one step of the history. The cards that have that value
 * already, templates and cards that are not there are left out; when none
 * is left, nothing is done. The server takes at most `CARD_BATCH_LIMIT`
 * cards in one request: more are refused, and nothing changes.
 */
export async function setCardsProperty(
  store: BoardStore,
  cardIds: readonly string[],
  propertyId: string,
  value: PropertyValue | undefined,
): Promise<void> {
  const forward: CardWrite[] = [];
  for (const cardId of new Set(cardIds)) {
    const card = store.card(cardId);
    if (!card || card.fields.isTemplate) continue;
    const [patch, restore] = cardPatches(withCardProperty(card, propertyId, value), card);
    if (!isEmptyCardPatch(patch)) forward.push({ cardId, patch, restore });
  }
  if (forward.length === 0) return;
  const backward = forward.map(({ cardId, patch, restore }) => ({
    cardId,
    patch: restore,
    restore: patch,
  }));
  let said = false;
  await performBoardAction(
    store,
    async () => {
      await writeCards(store, forward, said);
      said = true;
    },
    () => writeCards(store, backward, true),
    'changeProperty',
  );
}

/** Clears `propertyId` on the cards of `cardIds`, as one step of the history. */
export function clearCardsProperty(
  store: BoardStore,
  cardIds: readonly string[],
  propertyId: string,
): Promise<void> {
  return setCardsProperty(store, cardIds, propertyId, undefined);
}
