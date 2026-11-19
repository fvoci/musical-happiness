/**
 * The files going up for a body, a card's description or a wiki page's.
 *
 * Each is given up when what it was going up for closes: its block would be
 * put in nowhere. The reader is told so then, wherever they went, where the
 * shell tells of what failed: an upload that ends with the page it was for
 * would otherwise end without a word.
 */
import type { ProblemAction } from './workspace.svelte.ts';

export class BodyUploads {
  #underWay = $state.raw<AbortController[]>([]);
  readonly #report: (action: ProblemAction, error: unknown) => void;

  constructor(report: (action: ProblemAction, error: unknown) => void) {
    this.#report = report;
  }

  /** A file is going up: closing now would give it up. */
  get goingUp(): boolean {
    return this.#underWay.length > 0;
  }

  /** Runs one upload, given up by the signal it is handed. One that fails is told; one given up, by `giveUp`. */
  async run<T>(upload: (signal: AbortSignal) => Promise<T>): Promise<T> {
    const controller = new AbortController();
    this.#underWay = [...this.#underWay, controller];
    try {
      return await upload(controller.signal);
    } catch (error) {
      if (!controller.signal.aborted) this.#report('upload', error);
      throw error;
    } finally {
      this.#underWay = this.#underWay.filter((other) => other !== controller);
    }
  }

  /** What they were going up for closes: those under way are given up, and that is told, once. */
  giveUp(): void {
    if (this.#underWay.length === 0) return;
    for (const upload of this.#underWay) upload.abort();
    this.#report('left', undefined);
  }
}
