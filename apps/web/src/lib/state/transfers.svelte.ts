/**
 * The archives on their way down or up (`Workspace.exportBoard`,
 * `exportWorkspace`, `importArchive`), for the reader to watch: their
 * progress and their end.
 */
import type { ProblemAction } from './workspace.svelte.ts';

/** An archive on its way down or up, for the reader to watch. */
export interface Transfer {
  id: number;
  kind: 'export' | 'import';
  /** What is moved: the board's title, the workspace's, or the archive's file name. */
  name: string;
  done: number;
  /** 0 while the length is not known. */
  total: number;
  finished: boolean;
}

/** How long a finished transfer stays in sight, in milliseconds. */
const TRANSFER_STAYS = 6000;

export class Transfers {
  list = $state.raw<Transfer[]>([]);
  readonly #report: (kind: ProblemAction, error: unknown) => void;
  #count = 0;
  /** What stops each transfer under way, and what it moves (one at a time of each). */
  #running: { id: number; stop: AbortController; key: string }[] = [];

  /** `report` tells a failure, as a problem of the kind of the transfer. */
  constructor(report: (kind: ProblemAction, error: unknown) => void) {
    this.#report = report;
  }

  /**
   * Runs a transfer that the reader can watch: its progress, its end. A
   * finished one stays a moment, then goes; a failed one is told as a
   * problem; one given up (dismissed while under way) is told nothing. The
   * same thing is not moved twice at once: asked again meanwhile, nothing
   * more is done.
   */
  async run<T>(
    kind: Transfer['kind'],
    key: string,
    name: string,
    run: (options: {
      onProgress: (progress: { sentBytes: number; totalBytes: number }) => void;
      signal: AbortSignal;
    }) => Promise<T>,
  ): Promise<T | undefined> {
    if (this.#running.some((running) => running.key === key)) return undefined;
    this.#count += 1;
    const id = this.#count;
    const stop = new AbortController();
    this.#running = [...this.#running, { id, stop, key }];
    const change = (patch: Partial<Transfer>): void => {
      this.list = this.list.map((one) => (one.id === id ? { ...one, ...patch } : one));
    };
    this.list = [...this.list, { id, kind, name, done: 0, total: 0, finished: false }];
    try {
      const result = await run({
        onProgress: (progress) => {
          change({ done: progress.sentBytes, total: progress.totalBytes });
        },
        signal: stop.signal,
      });
      if (stop.signal.aborted) return undefined;
      change({ finished: true });
      setTimeout(() => {
        this.dismiss(id);
      }, TRANSFER_STAYS);
      return result;
    } catch (error) {
      if (!stop.signal.aborted) {
        this.dismiss(id);
        this.#report(kind, error);
      }
      return undefined;
    } finally {
      this.#running = this.#running.filter((running) => running.id !== id);
    }
  }

  /** Takes a transfer out of sight; one under way is given up. */
  dismiss(id: number): void {
    this.#running.find((running) => running.id === id)?.stop.abort();
    this.list = this.list.filter((one) => one.id !== id);
  }
}
