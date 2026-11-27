/**
 * The quick filters under the header of a view, over the real stores and
 * the in-memory backend: the chips a board offers, what pressing one does
 * to the cards of the view and to nothing else, the button that turns them
 * off, and a card the reader makes while they are on.
 */
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import { QUICK_FILTERS_KEY, quickFilters } from '$lib/state/board-quick.svelte.ts';
import type { BoardStore } from '$lib/state/index.ts';

import { openFirstBoard, OWNER, setup, STATUS } from '../../state/support.ts';

import Harness from './Harness.svelte';

let shown: BoardStore | undefined;
afterEach(() => {
  if (shown) quickFilters.clear(shown.boardId, 'browser');
  shown = undefined;
  localStorage.clear();
  cleanup();
});

/** The small board, given a rule of done (the option "Done") and a due date. */
async function mount(): Promise<BoardStore> {
  const { board } = await openFirstBoard(setup());
  const definition = board.board;
  if (!definition) throw new Error('no board');
  board.board = {
    ...definition,
    cardProperties: [
      ...definition.cardProperties,
      { id: 'due', name: 'Due', type: 'date', options: [] },
    ],
    doneWhen: { kind: 'option', propertyId: STATUS, optionIds: ['done'] },
    dueDatePropertyId: 'due',
  };
  shown = board;
  // The seed gives its views identifiers of its own: the view of the small board is "Board".
  viewId = board.views.find((view) => view.title === 'Board')?.id ?? '';
  render(Harness, { props: { board, viewId } });
  return board;
}
let viewId = '';

const row = (): HTMLElement => screen.getByTestId('quick-filters');
const chip = (name: string): HTMLElement => within(row()).getByRole('button', { name });
const titles = (board: BoardStore): string[] => {
  const view = board.view(viewId);
  return view ? board.filteredCardsOf(view).map((card) => card.title) : [];
};

describe('the quick filters', () => {
  it('are the chips the board has what for, none pressed, in a row of their own', async () => {
    await mount();
    expect(row()).toHaveAccessibleName('Quick filters');
    expect(screen.getByTestId('view-header')).not.toContainElement(row());
    const chips = within(row()).getAllByRole('button');
    expect(chips.map((each) => each.textContent.trim())).toEqual([
      'Mine',
      'Not done',
      'Overdue',
      'Due this week',
    ]);
    for (const each of chips) expect(each).toHaveAttribute('aria-pressed', 'false');
    // Nothing of them is a tool of the view.
    expect(row().querySelector('[data-tool]')).toBeNull();
  });

  it('narrow the cards of the view as they are pressed, and leave the view as it was', async () => {
    const board = await mount();
    board.cards = board.cards.map((card) =>
      card.title === 'Gamma'
        ? { ...card, fields: { ...card.fields, properties: { [STATUS]: 'done' } } }
        : card,
    );
    const before = board.view(viewId);
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Gamma']);

    await fireEvent.click(chip('Not done'));
    expect(chip('Not done')).toHaveAttribute('aria-pressed', 'true');
    expect(titles(board)).toEqual(['Alpha', 'Beta']);
    expect(board.view(viewId)).toBe(before);
    expect(localStorage.getItem(QUICK_FILTERS_KEY + board.boardId)).toBe('["incomplete"]');

    await fireEvent.click(chip('Overdue'));
    expect(titles(board)).toEqual([]);
    const clear = within(row()).getByRole('button', {
      name: 'Clear the 2 quick filters that are on',
    });
    expect(clear).toHaveTextContent('2');

    await fireEvent.click(clear);
    expect(chip('Not done')).toHaveAttribute('aria-pressed', 'false');
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Gamma']);
    expect(within(row()).queryByRole('button', { name: /^Clear/ })).toBeNull();
    expect(localStorage.getItem(QUICK_FILTERS_KEY + board.boardId)).toBeNull();
  });

  it('let a card the reader makes while they are on stay, without values given to meet them', async () => {
    const board = await mount();
    await fireEvent.click(chip('Mine'));
    const mine = titles(board);
    expect(mine.length).toBeLessThan(3);
    const made = await board.createCard({ title: 'Fresh' });
    expect(made?.fields.properties[OWNER]).toBeUndefined();
    expect(titles(board)).toEqual([...mine, 'Fresh']);
  });

  it('are offered without "Not done" and the due ones on a board that has no rule and no due date', async () => {
    const board = await mount();
    const definition = board.board;
    if (!definition) throw new Error('no board');
    await fireEvent.click(chip('Overdue'));
    board.board = { ...definition, doneWhen: null, dueDatePropertyId: null };
    await Promise.resolve();
    expect(
      within(row())
        .getAllByRole('button')
        .map((each) => each.textContent.trim()),
    ).toEqual(['Mine']);
    // The chip that is not shown narrows nothing.
    expect(titles(board)).toEqual(['Alpha', 'Beta', 'Gamma']);
  });
});
