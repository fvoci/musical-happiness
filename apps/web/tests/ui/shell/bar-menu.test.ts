/**
 * What the app lends the top bar's menu of a narrow pane: each offer is there
 * from when it is lent until it is taken back, in the order lent, and what
 * the bar reads of it follows it.
 */
import { describe, expect, it } from 'vitest';

import { BarMenu, type BarOffer } from '$lib/features/shell/bar-menu.svelte.ts';

function offerOf(id: string, mark?: string): BarOffer {
  return {
    line: { id, label: id, run: () => undefined },
    mark,
    markLabel: id,
  };
}

describe('the bar menu', () => {
  it('holds what is lent until it is taken back, in the order lent', () => {
    const menu = new BarMenu();
    const first = offerOf('first');
    const second = offerOf('second', '3');
    const takeFirst = menu.offer(first);
    const takeSecond = menu.offer(second);
    expect(menu.offers).toEqual([first, second]);
    takeFirst();
    expect(menu.offers).toEqual([second]);
    takeFirst();
    expect(menu.offers).toEqual([second], 'taken back twice, gone once');
    takeSecond();
    expect(menu.offers).toEqual([]);
  });

  it('reads an offer as it is now', () => {
    const menu = new BarMenu();
    let unread = 0;
    menu.offer({
      line: { id: 'notifications', label: 'Notifications', run: () => undefined },
      get mark() {
        return unread > 0 ? String(unread) : undefined;
      },
      markLabel: 'Notifications',
    });
    expect(menu.offers[0]?.mark).toBeUndefined();
    unread = 2;
    expect(menu.offers[0]?.mark).toBe('2');
  });
});
