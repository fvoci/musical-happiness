/**
 * Places of a list put in order by hand: the server's orders of a wiki's pages
 * (crates/model/src/page.rs, the same cases), and the writes that move one
 * thing of a list, give a copy its place and a new thing the last.
 */
import { describe, expect, it } from 'vitest';

import {
  isPlace,
  movedTo,
  placeAfter,
  placeAtEnd,
  placeBetween,
  spreadPlaces,
  type Placed,
} from '$lib/logic/list-places.ts';

const between = (before?: string, after?: string): string => {
  const made = placeBetween(before, after);
  if (made === undefined) throw new Error(`no place between ${before} and ${after}`);
  return made;
};

/** A sequence of numbers that is the same in every run (xorshift, as the server's test). */
function sequence(seed: bigint): (limit: number) => number {
  let state = seed;
  const mask = (1n << 64n) - 1n;
  return (limit) => {
    state ^= (state << 13n) & mask;
    state ^= state >> 7n;
    state ^= (state << 17n) & mask;
    return Number(state % BigInt(limit));
  };
}

describe('a place between two', () => {
  it('leaves room on both sides of the first', () => {
    const first = between();
    expect(first).toBe('V');
    expect(between(undefined, first) < first).toBe(true);
    expect(between(first, undefined) > first).toBe(true);
  });

  it('is between the two wherever it is asked for', () => {
    const below = sequence(0x2545f4914f6cdd1dn);
    const places: string[] = [];
    for (let turn = 0; turn < 3000; turn += 1) {
      const at = below(places.length + 1);
      const before = places[at - 1];
      const after = places[at];
      const made = between(before, after);
      expect(before === undefined || before < made).toBe(true);
      expect(after === undefined || made < after).toBe(true);
      expect(isPlace(made)).toBe(true);
      places.splice(at, 0, made);
    }
    expect([...places].sort()).toEqual(places);
  });

  it('is found between two next to each other, as the server finds it', () => {
    for (const [before, after] of [
      ['1', '2'],
      ['V', 'V1'],
      ['Vz', 'W'],
      ['01', '1'],
      ['z', 'z1'],
    ] as const) {
      const made = between(before, after);
      expect(before < made && made < after).toBe(true);
    }
    expect(between(undefined, '1')).toBe('0z');
    expect(between(undefined, '01')).toBe('00z');
    expect(between('V', 'W')).toBe('VV');
    expect(between('z', undefined)).toBe('z1');
  });

  it('stays short for a thousand added at one end', () => {
    let [last, first] = [between(), between()];
    for (let turn = 0; turn < 1000; turn += 1) {
      last = between(last, undefined);
      first = between(undefined, first);
    }
    expect(last.length).toBeLessThanOrEqual(20);
    expect(first.length).toBeLessThanOrEqual(20);
  });

  it('is none for what is no place, or for two not in order', () => {
    for (const malformed of ['', 'V0', 'a-b', 'ö']) {
      expect(isPlace(malformed)).toBe(false);
      expect(placeBetween(malformed, undefined)).toBeUndefined();
      expect(placeBetween(undefined, malformed)).toBeUndefined();
    }
    expect(isPlace(3)).toBe(false);
    expect(placeBetween('V', 'V')).toBeUndefined();
    expect(placeBetween('W', 'V')).toBeUndefined();
  });

  it('spreads a count of places short and evenly, with room between and at the ends', () => {
    for (const count of [1, 2, 5, 61, 62, 200]) {
      const spread = spreadPlaces(count);
      expect(spread).toHaveLength(count);
      expect(spread.every((place) => isPlace(place))).toBe(true);
      expect([...spread].sort()).toEqual(spread);
      expect(new Set(spread).size).toBe(count);
      expect(placeBetween(undefined, spread[0])).toBeDefined();
      expect(placeBetween(spread.at(-1), undefined)).toBeDefined();
    }
    expect(spreadPlaces(1)).toEqual(['V']);
    expect(spreadPlaces(200).every((place) => place.length <= 2)).toBe(true);
  });
});

const list = (...items: [string, string?][]): Placed[] =>
  items.map(([id, place]) => ({ id, place }));

describe('moving one thing of a list', () => {
  it('writes the thing moved alone where every one has a place', () => {
    const shown = list(['a', 'V'], ['b', 'W'], ['c', 'X']);
    expect(movedTo(shown, 'c', 1)).toEqual([{ id: 'c', place: 'VV' }]);
    expect(movedTo(shown, 'a', 1)).toEqual([{ id: 'a', place: 'WV' }]);
    expect(movedTo(shown, 'b', 0)).toEqual([{ id: 'b', place: 'U' }]);
    expect(movedTo(shown, 'b', 2)).toEqual([{ id: 'b', place: 'Y' }]);
  });

  it('gives those without a place the places they show at, the first time', () => {
    // Nobody moved anything yet: a, b, c, d by title.
    const shown = list(['a'], ['b'], ['c'], ['d']);
    expect(movedTo(shown, 'c', 1)).toEqual([
      { id: 'a', place: 'V' },
      { id: 'b', place: 'W' },
      { id: 'd', place: 'Y' },
      { id: 'c', place: 'VV' },
    ]);
    // Some placed, some not: those after the placed ones go on from the last of them.
    expect(movedTo(list(['p', 'V'], ['a'], ['b']), 'p', 1)).toEqual([
      { id: 'a', place: 'W' },
      { id: 'b', place: 'X' },
      { id: 'p', place: 'WV' },
    ]);
  });

  it('gives the same places for the same list, whoever moves what', () => {
    const shown = list(['a'], ['b'], ['c'], ['d']);
    const mine = movedTo(shown, 'c', 1);
    const theirs = movedTo(shown, 'a', 3);
    const placeOf = (writes: { id: string; place: string }[], id: string) =>
      writes.find((write) => write.id === id)?.place;
    // b is moved by neither: both write it the same place.
    expect(placeOf(mine, 'b')).toBe(placeOf(theirs, 'b'));
    expect(placeOf(mine, 'd')).toBe('Y');
    expect(placeOf(theirs, 'd')).toBe('Y');
  });

  it('numbers the list anew where two of one place leave no room between them', () => {
    // b and c have one place (two moved there at once): the id settles them.
    const shown = list(['a', 'V'], ['b', 'W'], ['c', 'W'], ['d', 'X']);
    const writes = movedTo(shown, 'd', 2);
    const after = new Map(shown.map((item) => [item.id, item.place]));
    for (const { id, place } of writes) after.set(id, place);
    const order = [...after].sort(([idA, a = ''], [idB, b = '']) =>
      a < b ? -1 : a > b ? 1 : idA < idB ? -1 : 1,
    );
    expect(order.map(([id]) => id)).toEqual(['a', 'b', 'd', 'c']);
    expect(new Set(writes.map((write) => write.place)).size).toBe(writes.length);
  });
});

describe('a copy and a new thing', () => {
  it('puts a copy right after its thing, or gives it no place where that has none', () => {
    const shown = list(['a', 'V'], ['b', 'W'], ['c']);
    expect(placeAfter(shown, 'a')).toBe('VV');
    // Before the next, though it has no place yet.
    expect(placeAfter(shown, 'b')).toBe('WV');
    expect(placeAfter(shown, 'c')).toBeUndefined();
    // No room after it: its own, the id settles them.
    expect(placeAfter(list(['a', 'V'], ['b', 'V']), 'a')).toBe('V');
  });

  it('puts a new thing last where the last has a place, and gives it none on a list without', () => {
    expect(placeAtEnd(list(['a', 'V'], ['b', 'W']))).toBe('X');
    expect(placeAtEnd(list(['a'], ['b']))).toBeUndefined();
    expect(placeAtEnd([])).toBeUndefined();
  });
});
