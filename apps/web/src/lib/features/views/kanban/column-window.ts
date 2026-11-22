/**
 * A window on a long column: which of its cards are drawn, and the blank
 * that stands for each run of those that are not.
 *
 * A column of a thousand cards drawn whole is most of the cost of drawing
 * the board and of scrolling it. Past `WINDOW_FROM` cards a column draws
 * those near what is in sight, and besides them, wherever they are, the
 * cards near the focus, near the gap of a drag and the card that is open;
 * the cards on their way are drawn too, hidden (KanbanColumn). Each run of
 * cards left out is one blank as high as they would be: as high as each was
 * when it was last drawn, or as its face is guessed to be.
 *
 * Cards are counted here as the drag counts them: among those that are not
 * on their way (`at`).
 */
import { faceProperties } from '$lib/components/card-face/face.ts';
import type { Card } from '$lib/logic/wire.ts';

import { badgesOf, type FaceContext } from './board-data.ts';

/** A column of more cards than this is drawn in part. */
export const WINDOW_FROM = 60;
/** The room between two cards, and around them in the list (`gap-2`, `p-2`). */
export const CARD_GAP = 8;
const PADDING = 8;
/** Cards drawn on either side of the focus, the gap and the open card. */
export const NEAR = 3;

/** How high a card is taken to be before it has been drawn: its frame and title, its lines, its badges. */
export function guessHeight(
  card: Card,
  face: Pick<FaceContext, 'templates' | 'visiblePropertyIds'>,
): number {
  const rows = faceProperties(card, face.templates, face.visiblePropertyIds).length;
  const badges = badgesOf(card);
  const hasBadges = badges.hasBody || badges.tasksTotal > 0;
  // As a card measures in either density: 50px of frame and title, 34px a line, 24px of badges.
  return 50 + rows * 34 + (rows > 0 ? 4 : 0) + (hasBadges ? 24 : 0);
}

/** Where each card starts, from the top of the list's content, and after them where the last ends. */
export function topsOf(heights: readonly number[]): number[] {
  const tops: number[] = [];
  let y = PADDING;
  for (const height of heights) {
    tops.push(y);
    y += height + CARD_GAP;
  }
  tops.push(y);
  return tops;
}

/** The cards, from `first` up to `end`, that reach between `from` and `to` pixels down the content. */
export function cardsBetween(
  tops: readonly number[],
  from: number,
  to: number,
): { first: number; end: number } {
  const count = tops.length - 1;
  // The first card whose bottom is below `from`, and the first whose top is at `to` or below.
  const first = search(count, (at) => (tops[at + 1] ?? 0) - CARD_GAP > from);
  const end = search(count, (at) => (tops[at] ?? 0) >= to);
  return { first, end: Math.max(first, end) };
}

/** The first of `count` places for which `past` holds, `past` holding from some place on; `count` if none. */
function search(count: number, past: (at: number) => boolean): number {
  let low = 0;
  let high = count;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (past(middle)) high = middle;
    else low = middle + 1;
  }
  return low;
}

/** A run of cards that are drawn, as `first` up to `end`. */
export interface Run {
  first: number;
  end: number;
}

/** The runs of cards drawn: those in sight, and those near each of the `pins`, within `count`. */
export function drawnRuns(count: number, inSight: Run, pins: readonly number[]): Run[] {
  const runs = [
    inSight,
    ...pins.filter((pin) => pin >= 0).map((pin) => ({ first: pin - NEAR, end: pin + NEAR + 1 })),
  ]
    .map((run) => ({ first: Math.max(0, run.first), end: Math.min(count, run.end) }))
    .filter((run) => run.first < run.end)
    .sort((one, other) => one.first - other.first);
  const merged: Run[] = [];
  for (const run of runs) {
    const last = merged.at(-1);
    if (last && run.first <= last.end) last.end = Math.max(last.end, run.end);
    else merged.push({ ...run });
  }
  return merged;
}

/** What the list shows, in order. */
export type Piece =
  /** `at`: its place among the cards not on their way, -1 for one on its way; `position`: in the column. */
  | { kind: 'card'; card: Card; at: number; position: number }
  | { kind: 'gap' }
  /**
   * Stands for cards not drawn; `cards` of them, 0 for the blank always at
   * either end of a window (`key` `start`, `end`), which then takes back the
   * room the list leaves after it or before it.
   */
  | { kind: 'blank'; key: string; height: number; cards: number };

const GAP: Piece = { kind: 'gap' };

/**
 * The list: the cards drawn, those on their way among them, the gap at `gap`
 * (-1 for none) and a blank for each run of the others. A card on its way
 * is hidden and takes no room, nor the room around it, so a blank on either
 * side of one adds up to what the two runs would be.
 *
 * A window begins and ends with a blank, empty or not, always the same two:
 * were the one at the end taken out while the window moves, the list would
 * lose its height, and a browser may put its scroll back to the top before
 * the new one comes in.
 */
export function piecesOf(
  cards: readonly Card[],
  dragged: readonly string[],
  gap: number,
  runs: readonly Run[] | undefined,
  heights: readonly number[],
): Piece[] {
  const pieces: Piece[] = [];
  let at = 0;
  let blank: { at: number; height: number; cards: number } | undefined;
  const close = (): void => {
    if (!blank) return;
    const height = blank.height + CARD_GAP * (blank.cards - 1);
    pieces.push({ kind: 'blank', key: String(blank.at), height, cards: blank.cards });
    blank = undefined;
  };
  let run = 0;
  for (const [position, card] of cards.entries()) {
    if (dragged.includes(card.id)) {
      close();
      pieces.push({ kind: 'card', card, at: -1, position });
      continue;
    }
    if (at === gap) {
      close();
      pieces.push(GAP);
    }
    while (runs && (runs[run]?.end ?? Infinity) <= at) run += 1;
    const current = runs?.[run];
    if (!runs || (current && current.first <= at && at < current.end)) {
      close();
      pieces.push({ kind: 'card', card, at, position });
    } else {
      blank ??= { at, height: 0, cards: 0 };
      blank.height += heights[at] ?? 0;
      blank.cards += 1;
    }
    at += 1;
  }
  close();
  if (at === gap) pieces.push(GAP);
  if (!runs) return pieces;
  const first = pieces[0]?.kind === 'blank' ? pieces.shift() : undefined;
  const last = pieces.at(-1)?.kind === 'blank' ? pieces.pop() : undefined;
  return [edge(first, 'start'), ...pieces, edge(last, 'end')];
}

/** The blank at one end of a window: the one there, or an empty one. */
function edge(piece: Piece | undefined, key: 'start' | 'end'): Piece {
  return piece?.kind === 'blank' ? { ...piece, key } : { kind: 'blank', key, height: 0, cards: 0 };
}
