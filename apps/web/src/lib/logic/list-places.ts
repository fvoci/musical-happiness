/**
 * Places in a list that people put in order by hand: each thing keeps a
 * place of its own, so that moving one writes that one alone, and two people
 * moving two things at once both keep their moves.
 *
 * A place is the server's order of a wiki's pages (`order_between` and
 * `spread_orders` in crates/model): a fraction written in the digits
 * `0-9A-Za-z` without its last zeros. Places compare as strings, by code
 * unit and not by a locale, so every browser lists them alike; there is
 * always one between two that differ. At an end it steps by one digit,
 * between two it goes half way.
 *
 * A list may hold things without a place (made before places were kept):
 * they come after those with one. A move among them gives each of them the
 * place it shows at (`movedTo`), after which every move is one write.
 */

/** The digits of a place, least first: in the order of their code units. */
const DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
const BASE = DIGITS.length;
const TOP = BASE - 1;
/** The longest place given: past it, the list is numbered anew (as the server's `ORDER_MAX_CHARS`). */
const LONGEST = 32;

/** The digits of a place as their indices in `DIGITS`; undefined for what is no place. */
function digitsOf(place: string): number[] | undefined {
  // By code point: what is not one of the digits, of whatever length, is no place.
  const digits = Array.from(place, (char) => DIGITS.indexOf(char));
  if (digits.length === 0 || digits.includes(-1) || digits.at(-1) === 0) return undefined;
  return digits;
}

/** Whether a value is a place: a string `placeBetween` makes and takes. */
export function isPlace(value: unknown): value is string {
  return typeof value === 'string' && digitsOf(value) !== undefined;
}

/** A place above `low` with nothing above it: `low` up to its first digit below the top, that one up. */
function stepAbove(low: readonly number[]): number[] {
  let atTop = 0;
  while (low[atTop] === TOP) atTop += 1;
  return [...low.slice(0, atTop), (low[atTop] ?? 0) + 1];
}

/**
 * A place below `high` with nothing below it: `high` up to its first digit that is not
 * zero, that one down; below a one, a zero and the top, which leaves the next step its room.
 */
function stepBelow(high: readonly number[]): number[] {
  let zeros = 0;
  while (high[zeros] === 0) zeros += 1;
  // A place does not end in a zero, so there is a digit after them.
  const digit = high[zeros] ?? 1;
  return [...high.slice(0, zeros), ...(digit === 1 ? [0, TOP] : [digit - 1])];
}

const midpoint = (one: number, other: number): number => Math.floor((one + other) / 2);

/** A place between `low` and `high`, where `low` is less than `high`. */
function halfway(low: readonly number[], high: readonly number[]): number[] {
  const digitOf = (digits: readonly number[], at: number): number => digits[at] ?? 0;
  // Where the two first differ, `low` taken as going on in zeros.
  let at = 0;
  while (high[at] === digitOf(low, at)) at += 1;
  const [below, above] = [digitOf(low, at), digitOf(high, at)];
  const made = high.slice(0, at);
  if (above - below > 1) return [...made, midpoint(below, above)];
  // Digits that follow each other: `high` cut here is between the two.
  if (high.length > at + 1) return [...made, above];
  // And `high` ends here: the place goes on from `low`, half way to the top.
  const rest = low.slice(at + 1);
  let atTop = 0;
  while (rest[atTop] === TOP) atTop += 1;
  return [...made, below, ...rest.slice(0, atTop), midpoint(digitOf(rest, atTop) + 1, BASE)];
}

/**
 * A place after `before` (undefined: the first place) and before `after`
 * (undefined: the last place). Undefined when a bound is no place, or
 * `before` is not less than `after`.
 */
export function placeBetween(
  before: string | undefined,
  after: string | undefined,
): string | undefined {
  const low = before === undefined ? undefined : digitsOf(before);
  const high = after === undefined ? undefined : digitsOf(after);
  if ((before !== undefined && !low) || (after !== undefined && !high)) return undefined;
  if (before !== undefined && after !== undefined && before >= after) return undefined;
  let made: number[];
  if (low && high) made = halfway(low, high);
  else if (low) made = stepAbove(low);
  else if (high) made = stepBelow(high);
  else made = [BASE / 2];
  return made.map((digit) => DIGITS[digit]).join('');
}

/**
 * `count` places, each after the one before, spread evenly over the places
 * of as few digits as leave room between every two and at both ends.
 */
export function spreadPlaces(count: number): string[] {
  const slots = count + 1;
  let width = 1;
  while (BASE ** width < slots * 2) width += 1;
  const span = BASE ** width;
  return Array.from({ length: count }, (_, index) => {
    let value = Math.floor(((index + 1) * span) / slots);
    let place = '';
    for (let digit = 0; digit < width; digit += 1) {
      place = DIGITS.charAt(value % BASE) + place;
      value = Math.floor(value / BASE);
    }
    return place.replace(/0+$/, '');
  });
}

/** A thing of a list, in the order the list shows, and its place if it has one. */
export interface Placed {
  readonly id: string;
  readonly place: string | undefined;
}

/** A place to write: the thing, and the place it takes. */
export interface PlaceWrite {
  id: string;
  place: string;
}

/**
 * The places of a list as it shows, those without one (they come last)
 * given the places they show at: each a step after the one before. The same
 * list gives the same places in every browser.
 */
function shownPlaces(list: readonly Placed[]): string[] {
  let last: string | undefined;
  return list.map((item) => {
    last = item.place ?? placeBetween(last, undefined) ?? '';
    return last;
  });
}

/**
 * The places that move `id` to `to` in `list` (the list as it shows, `to`
 * the index it is to have): its own, between those it goes between, and,
 * where some have no place yet, theirs too, the places they show at. Where
 * there is no room left between the two (two of one place), or the place
 * would be too long, every thing is given a new place.
 */
export function movedTo(list: readonly Placed[], id: string, to: number): PlaceWrite[] {
  // Of the whole list as it shows, the moved one too: whoever moves what, a thing
  // without a place is given the same one.
  const shown = new Map(shownPlaces(list).map((place, index) => [list[index]?.id, place]));
  const others = list.filter((item) => item.id !== id);
  const at = Math.max(0, Math.min(to, others.length));
  const places = others.map((item) => shown.get(item.id) ?? '');
  const place = placeBetween(places[at - 1], places[at]);
  if (place === undefined || place.length > LONGEST) {
    const ids = others.map((item) => item.id);
    ids.splice(at, 0, id);
    const spread = spreadPlaces(ids.length);
    const now = new Map(list.map((item) => [item.id, item.place]));
    return ids.flatMap((each, index) => {
      const given = spread[index] ?? '';
      return now.get(each) === given ? [] : [{ id: each, place: given }];
    });
  }
  const given = others.flatMap((item, index) =>
    item.place === undefined ? [{ id: item.id, place: places[index] ?? '' }] : [],
  );
  return [...given, { id, place }];
}

/**
 * The place of a copy of `id` right after it, written with the copy alone:
 * undefined when `id` has no place (the copy has none either, and goes by
 * the list's other rule), its own where there is no room after it.
 */
export function placeAfter(list: readonly Placed[], id: string): string | undefined {
  const at = list.findIndex((item) => item.id === id);
  const own = list[at]?.place;
  if (own === undefined) return undefined;
  const made = placeBetween(own, shownPlaces(list)[at + 1]);
  return made !== undefined && made.length <= LONGEST ? made : own;
}

/**
 * The place of a new thing at the end, written with it alone: after the
 * last, where that has a place (and so all of them do); undefined for a list
 * that has none yet, or none there (the new thing goes by the other rule).
 */
export function placeAtEnd(list: readonly Placed[]): string | undefined {
  const last = list.at(-1)?.place;
  return last === undefined ? undefined : placeBetween(last, undefined);
}
