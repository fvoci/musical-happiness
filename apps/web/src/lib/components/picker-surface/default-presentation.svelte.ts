/**
 * A default for how pickers open: a sheet when the viewport is narrow, a
 * popover otherwise. The surface itself never looks at the viewport; a
 * caller that has nothing better to go by passes `current` to it.
 */
import { IsMobile } from '$lib/hooks/is-mobile.svelte.js';

import type { Presentation } from './presentation.ts';

export class PickerPresentation {
  readonly #narrow = new IsMobile();

  get current(): Presentation {
    return this.#narrow.current ? 'sheet' : 'popover';
  }
}
