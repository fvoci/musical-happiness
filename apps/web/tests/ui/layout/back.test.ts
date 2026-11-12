/**
 * "Back" closes what is open. An entry is added when something opens, taken
 * out again when it is closed another way, and left to the router when the
 * reader goes somewhere else from inside it.
 */
import { flushSync } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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
  history.replaceState(null, '', '/w/t/boards/b/v');
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
    expect(location.pathname).toBe('/w/t/boards/b/v');
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
    router.openBoard({ id: 'other', workspaceId: 't' });
    drawer.open = false;
    flushSync();
    await settle();
    expect(location.pathname).toBe('/w/t/boards/other');
    expect(drawer.closedByBack).toBe(0);

    // One step back is where the reader was before opening the drawer.

    history.back();
    await settle();
    expect(location.pathname).toBe('/w/t/boards/b/v');
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
    expect(location.pathname).toBe('/w/t/boards/b/v/c');
    // The next "back" closes the card.
    history.back();
    await settle();
    expect(router.route.cardId).toBeUndefined();
    stop();
  });
});

describe('opening right after closing', () => {
  // A step back happens later. An entry added before it arrived can be counted
  // from the wrong place by the browser, and the next thing to close then
  // takes the reader a step too far. So nothing is written while a step of
  // ours is on its way.
  it('adds the entry of the next one only when the step back of the first has arrived', async () => {
    const first = open();
    await settle();
    const pushed = vi.spyOn(history, 'pushState');
    const arrived = vi.fn();
    window.addEventListener('popstate', arrived, { once: true });

    first.open = false;
    const second = open();
    // The step back of the first is under way: nothing is added yet.
    expect(pushed).not.toHaveBeenCalled();
    expect(arrived).not.toHaveBeenCalled();

    await settle();
    expect(arrived).toHaveBeenCalledTimes(1);
    expect(pushed).toHaveBeenCalledTimes(1);
    expect(second.open).toBe(true);
    expect(second.closedByBack).toBe(0);

    // "Back" closes the second, and only that.
    history.back();
    await settle();
    expect(second.open).toBe(false);
    expect(second.closedByBack).toBe(1);
    expect(location.pathname).toBe('/w/t/boards/b/v');
    pushed.mockRestore();
  });

  it('adds nothing for one that closed again before its turn came, and takes nothing back for it', async () => {
    const first = open();
    await settle();
    const pushed = vi.spyOn(history, 'pushState');
    const back = vi.spyOn(history, 'back');

    first.open = false;
    const second = open();
    second.open = false;
    flushSync();
    await settle();
    expect(pushed).not.toHaveBeenCalled();
    // One step back: that of the first.
    expect(back).toHaveBeenCalledTimes(1);

    // Nothing of ours is left in the way of the reader's own "back".
    const third = open();
    await settle();
    history.back();
    await settle();
    expect(third.closedByBack).toBe(1);
    pushed.mockRestore();
    back.mockRestore();
  });

  it('keeps a card open when a sheet in it closes and another opens at once, and one "back" then closes the card', async () => {
    const router = new Router('');
    const stop = router.listen();
    router.openCard('c');
    const menu = open();
    await settle();

    menu.open = false;
    const next = open();
    await settle();
    next.open = false;
    flushSync();
    await settle();
    expect(router.route.cardId).toBe('c');
    expect(location.pathname).toBe('/w/t/boards/b/v/c');

    history.back();
    await settle();
    expect(router.route.cardId).toBeUndefined();
    expect(location.pathname).toBe('/w/t/boards/b/v');
    stop();
  });

  it('writes an address the reader went to only after the step back of a sheet that just closed', async () => {
    const router = new Router('');
    const stop = router.listen();
    const sheet = open();
    await settle();
    const pushed = vi.spyOn(history, 'pushState');

    sheet.open = false;
    flushSync();
    router.openView('other');
    // The screen follows at once; the address waits for the step back.
    expect(router.route.viewId).toBe('other');
    expect(pushed).not.toHaveBeenCalled();
    expect(location.pathname).toBe('/w/t/boards/b/v');

    await settle();
    expect(pushed).toHaveBeenCalledTimes(1);
    expect(location.pathname).toBe('/w/t/boards/b/other');
    expect(router.route.viewId).toBe('other');

    // One step back is the view the reader came from.
    history.back();
    await settle();
    expect(location.pathname).toBe('/w/t/boards/b/v');
    expect(router.route.viewId).toBe('v');
    pushed.mockRestore();
    stop();
  });

  it('closes a card that was opened and closed while its address was still waiting', async () => {
    const router = new Router('');
    const stop = router.listen();
    const sheet = open();
    await settle();

    sheet.open = false;
    flushSync();
    router.openCard('c');
    router.closeCard();
    await settle();
    await settle();
    expect(router.route.cardId).toBeUndefined();
    expect(location.pathname).toBe('/w/t/boards/b/v');
    stop();
  });
});
