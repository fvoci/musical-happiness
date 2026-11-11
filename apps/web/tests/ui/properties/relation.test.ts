/**
 * The value of a card relation property: other cards of the board, picked by
 * title, shown as chips that open the card. A card that is not there (deleted)
 * is not shown and is left out of the next value written.
 */
import { describe, expect, it, vi } from 'vitest';

import { createCard, RELATION_LIMIT, setCardProperty, type Card } from '$lib/logic/wire.ts';

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
  it('lists the other cards of the board by title, not the card itself', async () => {
    mount('relation', [], { cards: CARDS });
    await fireEvent.click(button());
    expect(rows()).toEqual(['Build the form', 'Design the form', 'Untitled']);
  });

  it('lists the picked cards first, marked, and finds an untitled card by that word', async () => {
    mount('relation', ['card-2'], { cards: CARDS });
    await fireEvent.click(button());
    expect(rows()).toEqual(['Design the form', 'Build the form', 'Untitled']);
    await type(search(), 'untit');
    expect(rows()).toEqual(['Untitled']);
    await type(search(), '');
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
    // The cards that are not picked are not offered.
    await type(search(), `Card ${String(RELATION_LIMIT + 1)}`);
    expect(rows()).toEqual([]);
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
    expect(screen.getByText('More cards: 2950. Type to narrow the list.')).toBeInTheDocument();
    await type(search(), 'Task 2999');
    expect(rows()).toEqual(['Task 2999']);
  });

  it('keeps every card of a value that names more than the limit when one is taken out', async () => {
    const many = cardsOf(
      Object.fromEntries(
        Array.from({ length: RELATION_LIMIT + 10 }, (_, at) => [
          `c${String(at)}`,
          `Card ${String(at)}`,
        ]),
      ),
    );
    const named = [...many.keys()];
    const { onChange } = mount('relation', named, { cards: many });
    await fireEvent.click(button());
    await fireEvent.click(screen.getByRole('button', { name: 'Remove Card 0' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith(named.slice(1));
  });

  it('does not show the card itself when its value names it, and leaves it out when written', async () => {
    const { container, onChange } = mount('relation', ['card-1', 'card-2'], { cards: CARDS });
    expect(chips(container)).toEqual(['Design the form']);
    await fireEvent.click(button());
    await fireEvent.click(within(list()).getByRole('option', { name: 'Build the form' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith(['card-2', 'card-3']);
  });
});

describe('a relation read as an order', () => {
  const heldBack = (): HTMLElement =>
    document.querySelector<HTMLElement>('[data-slot="relation-held-back"]') ??
    (() => {
      throw new Error('no line about held back cards');
    })();

  /** card-2 comes after the card under test, and card-3 after card-2. */
  function ordered(): Map<string, Card> {
    const cards = cardsOf({
      'card-1': 'A card',
      'card-2': 'Design the form',
      'card-3': 'Build the form',
      'card-4': 'Ship it',
    });
    const named = (id: string): Card => {
      const found = cards.get(id);
      if (!found) throw new Error(`no ${id}`);
      return found;
    };
    setCardProperty(named('card-2'), 'property-relation', ['card-1']);
    setCardProperty(named('card-3'), 'property-relation', ['card-2']);
    return cards;
  }

  it('does not offer a card that would close a loop, however far, and says how many', async () => {
    mount('relation', [], { cards: ordered(), preventLoops: true });
    await fireEvent.click(button());
    expect(rows()).toEqual(['Ship it']);
    expect(heldBack()).toHaveTextContent('Not offered: 2. Picking them would make a loop.');
    // Counted among what was typed: the card looked for is there, and held back.
    await type(search(), 'design');
    expect(rows()).toEqual([]);
    expect(heldBack()).toHaveTextContent('Not offered: 1. Picking them would make a loop.');
    await type(search(), 'ship');
    expect(rows()).toEqual(['Ship it']);
    expect(heldBack()).toHaveTextContent(/^$/);
  });

  it('says only that the value is full when it is', async () => {
    const cards = ordered();
    for (let at = 0; at < RELATION_LIMIT; at += 1) {
      const extra = createCard();
      extra.id = `extra-${String(at)}`;
      extra.title = `Extra ${String(at)}`;
      cards.set(extra.id, extra);
    }
    const picked = [...cards.keys()].filter((id) => id.startsWith('extra-'));
    mount('relation', picked, { cards, preventLoops: true });
    await fireEvent.click(button());
    expect(heldBack()).toHaveTextContent(
      `No more than ${String(RELATION_LIMIT)} cards can be picked.`,
    );
  });

  it('offers every card where the relation is not read as an order', async () => {
    mount('relation', [], { cards: ordered() });
    await fireEvent.click(button());
    expect(rows()).toEqual(['Build the form', 'Design the form', 'Ship it']);
  });

  it('marks a card on a loop that is there already, and lets it be taken out', async () => {
    const { container, onChange } = mount('relation', ['card-3', 'card-4'], {
      cards: ordered(),
      preventLoops: true,
      onOpenCard: vi.fn(),
    });
    expect(chips(container)).toEqual(['Build the form', 'Ship it']);
    expect(screen.getByRole('button', { name: 'Open Build the form, in a loop' })).toHaveAttribute(
      'data-loop',
    );
    expect(screen.getByRole('button', { name: 'Open Ship it' })).not.toHaveAttribute('data-loop');
    await fireEvent.click(button());
    expect(rows()).toEqual(['Build the form', 'Ship it']);
    await fireEvent.click(within(list()).getByRole('option', { name: 'Build the form' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith(['card-4']);
  });
});
