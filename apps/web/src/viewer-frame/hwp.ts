/**
 * HWP and HWPX (the word processor documents of Korea), with rhwp.
 *
 * rhwp is WebAssembly, and it runs in a worker of its own (hwp-worker.ts):
 * the file is checked, read, laid out and drawn there, and the frame's own
 * thread only puts what the worker drew into the page (hwp-view.ts). The
 * worker has a time for each thing it is asked (opening the file, drawing a
 * page); past it, or when the frame goes, it is stopped (`terminate()`) and
 * its memory with it.
 *
 * The worker starts before rhwp's bytes arrive (`virtual:rhwp-wasm`, a module
 * of the frame's bundle fetched the first time an HWP document is opened): the
 * frame cannot fetch them, as it cannot fetch anything.
 */
import HwpWorker from './hwp-worker.ts?worker&inline';
import type { HwpFormat } from './hwp-messages.ts';
import { ask, HwpView } from './hwp-view.ts';
import type { Render } from './renderer.ts';
import { WorkerClient, type WorkerLike } from './worker-client.ts';

/** How long the worker may take over one thing it is asked, in milliseconds. */
export const WORK_LIMIT = 30_000;

/** How the renderer starts: its worker, and rhwp's WebAssembly as hexadecimal text. */
export interface Start {
  worker(): WorkerLike;
  wasm(): Promise<string>;
  /** How long the worker may take over one thing, in milliseconds. */
  limit: number;
}

const START: Start = {
  worker: () => new HwpWorker(),
  wasm: async () => (await import('virtual:rhwp-wasm')).default,
  limit: WORK_LIMIT,
};

/** The renderer of HWP (a compound file) or HWPX (an archive). */
export function renderer(format: HwpFormat, start: Start = START): Render {
  return async (bytes, place, report, words) => {
    const client = new WorkerClient(start.worker(), start.limit);
    // The viewer takes the frame out of the page when it closes: the worker stops then too.
    window.addEventListener(
      'pagehide',
      () => {
        client.close();
      },
      { once: true },
    );
    try {
      const wasm = await start.wasm();
      const opened = await ask(client, { kind: 'open', format, bytes, wasm }, [bytes]);
      if (opened.kind !== 'opened') throw new Error('the HWP renderer answered out of turn');
      const view = new HwpView(client, opened.pages, place, report, words);
      // Shown once the reader sees a page: the time to draw it counts, and a first page that
      // cannot be drawn is a failure rather than a white box (one too large to draw says so).
      await view.drawFirst();
      report.shown(opened.pages.length);
      return view;
    } catch (error) {
      client.close();
      throw error;
    }
  };
}
