/**
 * The cards as a graph by one relation property: edges, identifiers of
 * deleted cards, loops (closing a new one, finding those that are there) and
 * dates out of order.
 */
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import {
  cardsClosingLoop,
  closesLoop,
  cutIds,
  daysOutOfOrder,
  edgesOnLoops,
  edgesOutOfOrder,
  edgesOutOfOrderByDays,
  isOutOfOrder,
  relationEdges,
  type Days,
  type RelationEdge,
} from '$lib/logic/dependencies.ts';
import {
  createCard,
  setCardProperty,
  type Card,
  type PropertyTemplate,
  type PropertyValue,
} from '$lib/logic/wire.ts';

const AFTER = 'after';
const DUE: PropertyTemplate = { id: 'due', name: 'Due', type: 'date', options: [] };

function card(id: string, after?: string[], due?: PropertyValue): Card {
  const made = createCard();
  made.id = id;
  if (after) setCardProperty(made, AFTER, after);
  if (due !== undefined) setCardProperty(made, 'due', due);
  return made;
}

const edge = (from: string, to: string): RelationEdge => ({ from, to });

/** A chain a → b → c …: each card names the one before it. */
function chain(length: number): RelationEdge[] {
  return Array.from({ length: length - 1 }, (_, at) =>
    edge(`n${String(at)}`, `n${String(at + 1)}`),
  );
}

describe('edges', () => {
  it('go from the card named to the card that names it', () => {
    const cards = [card('a'), card('b', ['a']), card('c', ['a', 'b'])];
    expect(relationEdges(cards, AFTER)).toEqual([edge('a', 'b'), edge('a', 'c'), edge('b', 'c')]);
  });

  it('leave out deleted cards, the card itself and a card named twice', () => {
    const cards = [card('a'), card('b', ['gone', 'b', 'a', 'a'])];
    expect(relationEdges(cards, AFTER)).toEqual([edge('a', 'b')]);
  });

  it('come from a relation value only: a text or another property gives none', () => {
    const odd = card('b');
    setCardProperty(odd, AFTER, 'a');
    setCardProperty(odd, 'other', ['a']);
    expect(relationEdges([card('a'), odd], AFTER)).toEqual([]);
  });

  it('cut identifiers are the ones that name no card that is there', () => {
    const ids = new Set(['a', 'b']);
    expect(cutIds(card('b', ['gone', 'a', 'lost']), AFTER, ids)).toEqual(['gone', 'lost']);
    expect(cutIds(card('a'), AFTER, ids)).toEqual([]);
  });
});

describe('closing a loop', () => {
  it('is refused for the edge that turns two cards around', () => {
    const edges = [edge('a', 'b')];
    expect(closesLoop(edges, 'b', 'a')).toBe(true);
    expect(closesLoop(edges, 'a', 'b')).toBe(false);
  });

  it('is refused for a card naming itself', () => {
    expect(closesLoop([], 'a', 'a')).toBe(true);
  });

  it('is found at the end of a long chain', () => {
    const edges = chain(20_000);
    expect(closesLoop(edges, 'n19999', 'n0')).toBe(true);
    expect(closesLoop(edges, 'n0', 'n19999')).toBe(false);
  });

  it('is found through one of several branches, and a second way forward is no loop', () => {
    // a → b → d, a → c → d, d → e
    const edges = [edge('a', 'b'), edge('a', 'c'), edge('b', 'd'), edge('c', 'd'), edge('d', 'e')];
    expect(closesLoop(edges, 'e', 'c')).toBe(true);
    expect(closesLoop(edges, 'e', 'a')).toBe(true);
    expect(closesLoop(edges, 'b', 'c')).toBe(false);
    expect(closesLoop(edges, 'a', 'e')).toBe(false);
    expect([...cardsClosingLoop(edges, 'b')].sort()).toEqual(['b', 'd', 'e']);
  });

  it('agrees with walking every path, on generated graphs', () => {
    const graph = fc.array(fc.tuple(fc.nat(7), fc.nat(7)), { maxLength: 20 });
    fc.assert(
      fc.property(graph, fc.nat(7), fc.nat(7), (pairs, from, to) => {
        const edges = pairs.map(([a, b]) => edge(String(a), String(b)));
        expect(closesLoop(edges, String(from), String(to))).toBe(
          from === to || reaches(edges, String(to), String(from)),
        );
      }),
    );
  });
});

/** Whether `to` can be reached from `from` by one edge or more, the slow way. */
function reaches(edges: readonly RelationEdge[], from: string, to: string): boolean {
  const seen = new Set<string>();
  const visit = (id: string): boolean =>
    edges.some((each) => {
      if (each.from !== id || seen.has(each.to)) return false;
      seen.add(each.to);
      return each.to === to || visit(each.to);
    });
  return visit(from);
}

describe('loops that are there', () => {
  it('are the edges whose two cards reach each other', () => {
    // a ⇄ b, then b → c → d → b, and e hangs off c.
    const edges = [
      edge('a', 'b'),
      edge('b', 'a'),
      edge('b', 'c'),
      edge('c', 'd'),
      edge('d', 'b'),
      edge('c', 'e'),
      edge('x', 'y'),
    ];
    expect(edgesOnLoops(edges)).toEqual(edges.slice(0, 5));
  });

  it('are found around a long ring without running out of stack', () => {
    const ring = [...chain(20_000), edge('n19999', 'n0')];
    expect(edgesOnLoops(ring)).toHaveLength(20_000);
    expect(edgesOnLoops(chain(20_000))).toEqual([]);
  });

  it('agree with walking every path, on generated graphs', () => {
    const graph = fc.array(fc.tuple(fc.nat(6), fc.nat(6)), { maxLength: 18 });
    fc.assert(
      fc.property(graph, (pairs) => {
        const edges = pairs.map(([a, b]) => edge(String(a), String(b)));
        const slow = edges.filter(({ from, to }) => from === to || reaches(edges, to, from));
        expect(edgesOnLoops(edges)).toEqual(slow);
      }),
    );
  });
});

describe('dates out of order', () => {
  const day = (from: string, to?: string): PropertyValue => (to ? { from, to } : { from });

  it('when the later card starts before the earlier one ends', () => {
    const before = card('a', undefined, day('2026-03-02', '2026-03-10'));
    expect(isOutOfOrder(before, card('b', ['a'], day('2026-03-09')), DUE, 'UTC')).toBe(true);
    expect(isOutOfOrder(before, card('b', ['a'], day('2026-03-11')), DUE, 'UTC')).toBe(false);
  });

  it('not when one ends and the other starts on the same day', () => {
    const before = card('a', undefined, day('2026-03-02', '2026-03-10'));
    expect(
      isOutOfOrder(before, card('b', ['a'], day('2026-03-10', '2026-03-12')), DUE, 'UTC'),
    ).toBe(false);
  });

  it('not when either card has no date', () => {
    expect(isOutOfOrder(card('a'), card('b', [], day('2026-03-01')), DUE, 'UTC')).toBe(false);
    expect(isOutOfOrder(card('a', [], day('2026-03-09')), card('b'), DUE, 'UTC')).toBe(false);
  });

  it('a value with a time counts by the day it falls on for the reader', () => {
    // 16:00 UTC on 10 March is 01:00 on 11 March in Seoul.
    const before = card('a', undefined, { from: Date.UTC(2026, 2, 10, 16) });
    const after = card('b', ['a'], day('2026-03-10'));
    expect(isOutOfOrder(before, after, DUE, 'UTC')).toBe(false);
    expect(isOutOfOrder(before, after, DUE, 'Asia/Seoul')).toBe(true);
  });

  it('a time on the later card, and a range of times that ends on the next day for the reader', () => {
    const before = card('a', undefined, day('2026-03-10'));
    // 15:30 UTC on 9 March is 00:30 on 10 March in Seoul.
    const after = card('b', ['a'], { from: Date.UTC(2026, 2, 9, 15, 30) });
    expect(isOutOfOrder(before, after, DUE, 'UTC')).toBe(true);
    expect(isOutOfOrder(before, after, DUE, 'Asia/Seoul')).toBe(false);
    // 10:00 to 16:00 UTC on 10 March ends at 01:00 on the 11th in Seoul.
    const evening = card('c', undefined, {
      from: Date.UTC(2026, 2, 10, 10),
      to: Date.UTC(2026, 2, 10, 16),
    });
    const next = card('d', ['c'], day('2026-03-10'));
    expect(isOutOfOrder(evening, next, DUE, 'UTC')).toBe(false);
    expect(isOutOfOrder(evening, next, DUE, 'Asia/Seoul')).toBe(true);
  });

  it('by the moment a card was made, for the kind that has no stored value', () => {
    const made: PropertyTemplate = { id: 'made', name: 'Made', type: 'createdTime', options: [] };
    const before = card('a');
    before.createAt = Date.UTC(2026, 2, 10, 16);
    const after = card('b', ['a']);
    after.createAt = Date.UTC(2026, 2, 10, 12);
    // Both on the 10th in UTC; in Seoul the first is on the 11th, the second on the 10th.
    expect(isOutOfOrder(before, after, made, 'UTC')).toBe(false);
    expect(isOutOfOrder(before, after, made, 'Asia/Seoul')).toBe(true);
  });

  it('are the edges of a relation whose cards are out of order', () => {
    const cards = [
      card('a', undefined, day('2026-03-05')),
      card('b', ['a'], day('2026-03-04')),
      card('c', ['a', 'b'], day('2026-03-05')),
    ];
    const byId = new Map(cards.map((each) => [each.id, each]));
    expect(edgesOutOfOrder(byId, relationEdges(cards, AFTER), DUE, 'UTC')).toEqual([
      edge('a', 'b'),
    ]);
  });

  it('are the same edges by days worked out already, a card without days never among them', () => {
    const span = (first: string, last: string): Days => ({ first, last }) as Days;
    const days: Record<string, Days> = {
      a: span('2026-03-01', '2026-03-05'),
      b: span('2026-03-04', '2026-03-09'),
      c: span('2026-03-05', '2026-03-05'),
    };
    // The same day is in order; so is anything with a card that has no days.
    expect(daysOutOfOrder(days.a, days.c)).toBe(false);
    expect(daysOutOfOrder(days.a, undefined)).toBe(false);
    const edges = [edge('a', 'b'), edge('a', 'c'), edge('b', 'c'), edge('a', 'none')];
    expect(edgesOutOfOrderByDays(edges, (id) => days[id])).toEqual([
      edge('a', 'b'),
      edge('b', 'c'),
    ]);
  });
});
