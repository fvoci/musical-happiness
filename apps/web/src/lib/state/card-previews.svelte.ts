/**
 * What a gallery shows of its cards' bodies: read when a gallery is shown,
 * and only then (`Backend.cardPreviews`), and kept current while it is by
 * the board's change messages, whose cards carry the body it is made from.
 */
import { SvelteMap, SvelteSet } from 'svelte/reactivity';

import type { CardPreview } from '../editor/card-preview.ts';

import type { Backend } from './backend.ts';
import { isCardBlock } from './backend.ts';

export class CardPreviews {
  readonly #backend: Backend;
  readonly #boardId: string;
  readonly #previews = new SvelteMap<string, CardPreview>();
  /** Cards told of since the reading began: what the reading says of them is older. */
  readonly #told = new SvelteSet<string>();
  #reading = 0;

  constructor(backend: Backend, boardId: string) {
    this.#backend = backend;
    this.#boardId = boardId;
  }

  /** The preview of a card, once read; none while it is not. */
  of(cardId: string): CardPreview | undefined {
    return this.#previews.get(cardId);
  }

  /** Reads the previews and follows the board. Returns the function that stops it. */
  start(): () => void {
    const stop = this.#backend.watchContainer(this.#boardId, (change) => {
      if (change.kind === 'blocks') {
        for (const block of change.blocks) {
          if (!isCardBlock(block) || !block.preview) continue;
          this.#told.add(block.id);
          this.#previews.set(block.id, block.preview);
        }
      } else if (change.kind === 'resync') void this.#read();
    });
    void this.#read();
    return () => {
      stop();
      this.#reading += 1;
    };
  }

  async #read(): Promise<void> {
    this.#reading += 1;
    const reading = this.#reading;
    this.#told.clear();
    try {
      const previews = await this.#backend.cardPreviews(this.#boardId);
      if (reading !== this.#reading) return;
      for (const [cardId, preview] of previews) {
        if (!this.#told.has(cardId)) this.#previews.set(cardId, preview);
      }
    } catch {
      // The gallery shows its cards without previews; the next reading may bring them.
    }
  }
}
