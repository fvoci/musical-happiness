/**
 * What the app lends the top bar's menu of a narrow pane ("…") from beyond the
 * shell: a line after the bar's own, and a mark on the menu's button, for what
 * the bar has no room for there (the bell of the notifications). The bar makes
 * one and sets it as the context of what it shows (`lendBarMenu`); whoever
 * offers takes it back when it goes.
 */
import { createContext, untrack } from 'svelte';

import type { MenuAction } from '$lib/components/menu/actions.ts';

/** What one part of the app lends the menu; its getters may be reactive. */
export interface BarOffer {
  /** Its line in the menu. */
  readonly line: MenuAction;
  /** What the menu's button shows of it, as a count is shown; none for nothing. */
  readonly mark: string | undefined;
  /** What the mark says, for whoever does not see it. */
  readonly markLabel: string;
}

export class BarMenu {
  /** What is lent now, in the order it was. */
  offers = $state.raw<readonly BarOffer[]>([]);

  /** Lends `offer` until the function returned is called. */
  offer(offer: BarOffer): () => void {
    this.offers = [...untrack(() => this.offers), offer];
    return () => {
      this.offers = untrack(() => this.offers).filter((one) => one !== offer);
    };
  }
}

export const [barMenu, lendBarMenu, hasBarMenu] = createContext<BarMenu>();
