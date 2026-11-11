/**
 * The value of a card relation property: other cards of the board, picked by
 * title, shown as chips that open the card. A card that is not there (deleted)
 * is not shown and is left out of the next value written.
 */
import { describe, expect, it, vi } from 'vitest';

import { createCard, RELATION_LIMIT, type Card } from '$lib/logic/wire.ts';

import { fireEvent, mount, press, screen, type, valueButton, within } from './support.ts';

function cardsOf(titles: Record<string, string>): Map<string, Card> {
  return new Map(
    Object.entries(titles).map(([id, title]) => {
      const card = createCard();
      card.id = id;
      card.title = title;
      return [id, card];
    }),
  );
}

/** The board: the card under test (`card-1`) and three others. */
const CARDS = cardsOf({
  'card-1': 'A card',
  'card-2': 'Design the form',
  'card-3': 'Build the form',
  'card-4': '',
});

const button = (): HTMLElement => valueButton('My relation');
const search = (): HTMLInputElement => screen.getByRole('combobox', { name: 'My relation' });
const list = (): HTMLElement => screen.getByRole('listbox', { name: 'My relation' });
const rows = (): string[] =>
  within(list())
    .queryAllByRole('option')
    .map((row) => row.textContent.trim());
const chips = (container: HTMLElement): string[] =>
  [
    ...container.querySelectorAll('[data-slot="property-relation"] [data-slot="relation-chip"]'),
  ].map((chip) => chip.textContent.trim());

describe('a relation property, closed', () => {
  it('shows the cards it names by title, an untitled one as such', () => {
    const { container } = mount('relation', ['card-3', 'card-4'], { cards: CARDS });
    expect(chips(container)).toEqual(['Build the form', 'Untitled']);
  });

  it('does not show a card that is not there, nor one named twice', () => {
    const { container } = mount('relation', ['card-gone', 'card-2', 'card-2'], { cards: CARDS });
    expect(chips(container)).toEqual(['Design the form']);
  });

  it('shows a placeholder when it names only cards that are not there', () => {
    const { container } = mount('relation', ['card-gone'], { cards: CARDS });
    expect(chips(container)).toEqual([]);
    expect(within(button()).getByText('Empty')).toBeInTheDocument();
  });

  it('is empty for a value that is not a list', () => {
    const { container } = mount('relation', 'card-2', { cards: CARDS });
    expect(chips(container)).toEqual([]);
  });

  it('opens a card by its chip, named for a screen reader', async () => {
    const onOpenCard = vi.fn();
    mount('relation', ['card-2'], { cards: CARDS, onOpenCard });
    await fireEvent.click(screen.getByRole('button', { name: 'Open Design the form' }));
    expect(onOpenCard).toHaveBeenCalledExactlyOnceWith('card-2');
  });

  it('read only: chips without a picker', () => {
    const { container } = mount('relation', ['card-2'], { cards: CARDS, readOnly: true });
    expect(chips(container)).toEqual(['Design the form']);
    expect(screen.queryByRole('button', { name: /^My relation/ })).toBeNull();
  });
});

describe('a relation property, opened', () => {
  it('lists the other cards of the board, not the card itself, and marks the picked ones', async () => {
    mount('relation', ['card-2'], { cards: CARDS });
    await fireEvent.click(button());
    expect(rows()).toEqual(['Design the form', 'Build the form', 'Untitled']);
    expect(within(list()).getByRole('option', { name: 'Design the form' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
  });

  it('narrows the list by title as the reader types, and picks with the keyboard', async () => {
    const { onChange } = mount('relation', [], { cards: CARDS });
    await fireEvent.click(button());
    await type(search(), 'build');
    expect(rows()).toEqual(['Build the form']);
    await press(search(), 'Enter');
    expect(onChange).toHaveBeenCalledExactlyOnceWith(['card-3']);
  });

  it('adds a card, and takes out one that is pressed again', async () => {
    const { onChange } = mount('relation', ['card-2'], { cards: CARDS });
    await fireEvent.click(button());
    await fireEvent.click(within(list()).getByRole('option', { name: 'Build the form' }));
    expect(onChange).toHaveBeenLastCalledWith(['card-2', 'card-3']);
    await fireEvent.click(within(list()).getByRole('option', { name: 'Design the form' }));
    expect(onChange).toHaveBeenLastCalledWith([]);
  });

  it('takes out a card with the button next to its title, and Backspace takes the last', async () => {
    const { onChange } = mount('relation', ['card-2', 'card-3'], { cards: CARDS });
    await fireEvent.click(button());
    await fireEvent.click(screen.getByRole('button', { name: 'Remove Design the form' }));
    expect(onChange).toHaveBeenLastCalledWith(['card-3']);
    await press(search(), 'Backspace');
    expect(onChange).toHaveBeenLastCalledWith(['card-2']);
  });

  it('leaves the cards that are not there out of the value it writes', async () => {
    const { onChange } = mount('relation', ['card-gone', 'card-2'], { cards: CARDS });
    await fireEvent.click(button());
    await fireEvent.click(within(list()).getByRole('option', { name: 'Build the form' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith(['card-2', 'card-3']);
  });

  it(`picks no more than ${String(RELATION_LIMIT)} cards, and says so`, async () => {
    const many = cardsOf(
      Object.fromEntries(
        Array.from({ length: RELATION_LIMIT + 2 }, (_, at) => [
          `c${String(at)}`,
          `Card ${String(at)}`,
        ]),
      ),
    );
    const picked = [...many.keys()].slice(0, RELATION_LIMIT);
    const { onChange } = mount('relation', picked, { cards: many });
    await fireEvent.click(button());
    expect(screen.getByRole('status')).toHaveTextContent(
      `No more than ${String(RELATION_LIMIT)} cards can be picked.`,
    );
    await type(search(), `Card ${String(RELATION_LIMIT + 1)}`);
    await press(search(), 'Enter');
    expect(onChange).not.toHaveBeenCalled();
    // Taking one out still works.
    await type(search(), 'Card 0');
    await press(search(), 'Enter');
    expect(onChange).toHaveBeenCalledExactlyOnceWith(picked.slice(1));
  });

  it('draws a limited number of rows on a board of thousands of cards, and says how many more', async () => {
    const many = cardsOf(
      Object.fromEntries(
        Array.from({ length: 3000 }, (_, at) => [`c${String(at)}`, `Task ${String(at)}`]),
      ),
    );
    mount('relation', [], { cards: many });
    await fireEvent.click(button());
    expect(rows()).toHaveLength(50);
    expect(screen.getByText('2950 more cards. Type to narrow the list.')).toBeInTheDocument();
    await type(search(), 'Task 2999');
    expect(rows()).toEqual(['Task 2999']);
  });
});
