/**
 * "Back" closes what is open. An entry is added when something opens, taken
 * out again when it is closed another way, and left to the router when the
 * reader goes somewhere else from inside it.
 */
import { flushSync } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { Router } from '../../../src/app/router.svelte.ts';

import { overlay, type Overlay } from './overlay-host.svelte.ts';

/** Lets a `history.back()` and its `popstate` happen. */
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 20));

let made: Overlay[] = [];
const open = (): Overlay => {
  const one = overlay();
  made.push(one);
  one.open = true;
  flushSync();
  return one;
};

beforeEach(() => {
  history.replaceState(null, '', '/board/b/v');
});

afterEach(async () => {
  for (const one of made) one.destroy();
  made = [];
  await settle();
});

describe('closing on back', () => {
  it('adds one entry when something opens, and "back" closes it', async () => {
    const before = history.length;
    const sheet = open();
    expect(history.length).toBe(before + 1);
    expect(location.pathname).toBe('/board/b/v');
    history.back();
    await settle();
    expect(sheet.open).toBe(false);
    expect(sheet.closedByBack).toBe(1);
  });

  it('takes its entry back out when it is closed another way', async () => {
    const sheet = open();
    sheet.open = false;
    flushSync();
    await settle();
    expect(sheet.closedByBack).toBe(0);
    // The next "back" is the reader's own: nothing of ours is closed by it.
    const other = open();
    other.open = false;
    flushSync();
    await settle();
    expect(other.closedByBack).toBe(0);
  });

  it('closes what was opened last first', async () => {
    const card = open();
    const sheet = open();
    history.back();
    await settle();
    expect([card.open, sheet.open]).toEqual([true, false]);
    history.back();
    await settle();
    expect(card.open).toBe(false);
  });
});

describe('going somewhere else from inside a drawer', () => {
  it('puts the new address in place of the drawer\'s entry, so "back" is one step', async () => {
    const router = new Router('');
    const stop = router.listen();
    const drawer = open();
    router.openBoard('other');
    drawer.open = false;
    flushSync();
    await settle();
    expect(location.pathname).toBe('/board/other');
    expect(drawer.closedByBack).toBe(0);

    // One step back is where the reader was before opening the drawer.

    history.back();
    await settle();
    expect(location.pathname).toBe('/board/b/v');
    expect(router.route).toMatchObject({ boardId: 'b', viewId: 'v' });
    stop();
  });

  it('leaves the address alone when a sheet over a card closes on "back"', async () => {
    const router = new Router('');
    const stop = router.listen();
    router.openCard('c');
    const sheet = open();
    history.back();
    await settle();
    expect(sheet.open).toBe(false);
    expect(router.route.cardId).toBe('c');
    expect(location.pathname).toBe('/board/b/v/c');
    // The next "back" closes the card.
    history.back();
    await settle();
    expect(router.route.cardId).toBeUndefined();
    stop();
  });
});
