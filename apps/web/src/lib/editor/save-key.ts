/**
 * The key that keeps a version of a body now (`isSaveKey`), in the editor of
 * a body (BodyEditor.svelte): it never opens the browser's dialog to save the
 * page, and asks the editor's owner to keep a version. A key held down asks
 * once.
 *
 * Pressed while a syllable is being composed (Hangul, kana), it asks once the
 * syllable is over, so that what is kept has it, and the composition is left
 * alone. Chrome reports the key before it ends the composition; Safari ends
 * it first.
 */
import { isSaveKey } from '$lib/keys.ts';

export class SaveKey {
  /** What asks; none for an editor that keeps no version. */
  readonly #save: () => (() => void) | undefined;
  /** The key was pressed during a syllable, and asks when it is over. */
  #waiting = false;

  constructor(save: () => (() => void) | undefined) {
    this.#save = save;
  }

  /** A key pressed in the editor; `composing` while a syllable is under way there. */
  down(event: KeyboardEvent, composing: boolean, platform?: string): void {
    if (!isSaveKey(event, platform)) return;
    event.preventDefault();
    if (event.repeat) return;
    if (composing) this.#waiting = true;
    else this.#save()?.();
  }

  /** The syllable is over: a key pressed during it asks now. */
  composed(): void {
    if (!this.#waiting) return;
    this.#waiting = false;
    this.#save()?.();
  }

  /** The editor goes away: a key pressed during a syllable asks nothing. */
  drop(): void {
    this.#waiting = false;
  }
}
