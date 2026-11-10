/**
 * Which proposal each part follows, worked out once from the address. A part
 * the screen is not about follows the proposal the document recommends, as
 * in the mobile mock-ups.
 */
import { CURRENT } from './current.ts';

const { screen, variant, opened } = CURRENT;
const second = variant === 'b';
const main = opened === 'main';

export const SHOW = {
  /** A pane too narrow for bars: a small grid over the cards of one day, or a list of days. */
  narrow: (screen === 'month' || screen === 'range') && second ? 'agenda' : 'grid',
  /** Where the cards of one day open from a grid with bars. */
  dayList: screen === 'more' && second ? 'panel' : 'popover',
  /** What makes a card on a day: pressing the cell, or a "+" in it. */
  create: screen === 'create' && !second ? 'cell' : 'plus',
  /** A card has just been made on the empty day and waits for its title. */
  creating: screen === 'create' && main,
  /** Where the cards without a date are. */
  undated: screen === 'display' && second ? 'strip' : 'drawer',
  undatedOpen: screen === 'display' && main,
  /** The list of date properties is open. */
  byOpen: screen === 'display' && opened === 'by',
  /** A card is drawn in mid-drag. */
  dragging: screen === 'move' && !second && main,
  /** The menu of a card, or the date picker it leads to, is open. */
  cardMenu:
    screen === 'move' && second ? (opened === 'menu' ? 'menu' : main ? 'pick' : 'none') : 'none',
  /** A long card is drawn as picked, with the handles that change its ends. */
  handles: screen === 'range',
  /** The cards of a day are open when the page loads. */
  dayOpen: screen === 'more' && main,
  tinted: CURRENT.tinted,
} as const;
