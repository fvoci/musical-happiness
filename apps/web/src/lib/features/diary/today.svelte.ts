/**
 * Today on the reader's device: the day it is in their time zone (the
 * account's setting, else the browser's: `Workspace.regional`). The server
 * never says what today is (`state/diary-types.ts`).
 *
 * Read in an effect or the template, it is read again at the next midnight
 * of that time zone, and whenever the page is shown again after it was
 * hidden: a timer of a tab in the background may come late, or a device that
 * slept not at all.
 */
import { on } from 'svelte/events';
import { createSubscriber } from 'svelte/reactivity';

import { addDays, dateOfInstant, startOfDay, type CalendarDate } from '$lib/logic/dates.ts';

export class DeviceToday {
  readonly #timeZone: () => string;
  readonly #now: () => number;
  readonly #subscribe: () => void;

  constructor(timeZone: () => string, now: () => number = Date.now) {
    this.#timeZone = timeZone;
    this.#now = now;
    this.#subscribe = createSubscriber((update) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const wait = (): void => {
        clearTimeout(timer);
        const zone = this.#timeZone();
        const next = startOfDay(addDays(dateOfInstant(this.#now(), zone), 1), zone);
        timer = setTimeout(
          () => {
            update();
            wait();
          },
          Math.max(0, next - this.#now()),
        );
      };
      wait();
      const off = on(document, 'visibilitychange', () => {
        if (document.visibilityState !== 'visible') return;
        update();
        wait();
      });
      return () => {
        clearTimeout(timer);
        off();
      };
    });
  }

  get date(): CalendarDate {
    this.#subscribe();
    return dateOfInstant(this.#now(), this.#timeZone());
  }
}
