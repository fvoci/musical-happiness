/**
 * HWP and HWPX (the word processor documents of Korea), with rhwp.
 *
 * rhwp is WebAssembly, and it runs in a worker of its own (hwp-worker.ts):
 * the file is checked, read, laid out and drawn there, and the frame's own
 * thread only puts what the worker drew into the page. The worker has a time
 * for each thing it is asked (opening the file, drawing a page); past it, or
 * when the frame goes, it is stopped (`terminate()`) and its memory with it.
 *
 * The worker starts before rhwp's bytes arrive (`virtual:rhwp-wasm`, a module
 * of the frame's bundle fetched the first time an HWP document is opened): the
 * frame cannot fetch them, as it cannot fetch anything.
 *
 * The pages are laid out as boxes of their size at once, and only those in
 * view (and a screen's height around them) are drawn: the worker draws a page
 * as SVG, which hwp-page.ts copies into the box with the page's text over it.
 * A page that scrolls far away gives its picture back.
 */
import HwpWorker from './hwp-worker.ts?worker&inline';
import type { FromWorker, HwpFormat, PageSize, ToWorker } from './hwp-messages.ts';
import { pagePicture, pageText } from './hwp-page.ts';
import {
  LockedFile,
  PageFollower,
  SlowRenderer,
  UnreadableFile,
  type Drawn,
  type Render,
  type Report,
} from './renderer.ts';
import { WorkerClient, WorkerFailed } from './worker-client.ts';

/** How long the worker may take over one thing it is asked, in milliseconds. */
export const WORK_LIMIT = 30_000;
/** The room around the pages, in CSS pixels, at each side. */
const GUTTER = 16;

function failure(reason: Extract<FromWorker, { kind: 'failed' }>['reason']): Error {
  if (reason === 'locked') return new LockedFile();
  return reason === 'unreadable' ? new UnreadableFile() : new Error('the HWP renderer failed');
}

/** Asks the worker; resolves with its answer, rejects with what it says went wrong. */
async function ask(
  client: WorkerClient,
  message: ToWorker,
  transfer: Transferable[] = [],
  signal?: AbortSignal,
): Promise<FromWorker> {
  const answer = (await client.request(message, transfer, signal)) as FromWorker;
  if (answer.kind === 'failed') throw failure(answer.reason);
  return answer;
}

/** One page: its box in the column, and what is drawn in it while it is in view. */
class Page {
  readonly box = document.createElement('div');
  readonly number: number;
  readonly size: PageSize;
  /** The draw under way or done, until the page is cleared. */
  #drawing: Promise<void> | undefined;
  #giveUp: AbortController | undefined;

  constructor(number: number, size: PageSize) {
    this.number = number;
    this.size = size;
    this.box.className = 'page';
    this.box.dataset.page = String(number);
  }

  draw(client: WorkerClient): Promise<void> {
    if (this.#drawing) return this.#drawing;
    const giveUp = new AbortController();
    this.#giveUp = giveUp;
    this.#drawing = ask(client, { kind: 'page', page: this.number }, [], giveUp.signal)
      .then((answer) => {
        if (giveUp.signal.aborted || answer.kind !== 'page') return;
        const picture = pagePicture(answer.svg, this.number, document);
        if (!picture) throw new UnreadableFile();
        const text = pageText(answer.text, document);
        text.style.width = `${this.size.width}px`;
        text.style.height = `${this.size.height}px`;
        this.box.replaceChildren(picture, text);
      })
      .catch((error: unknown) => {
        // A later draw of the page, once it came back into view, is not this one's to clear.
        if (this.#giveUp === giveUp) this.clear();
        throw error;
      });
    return this.#drawing;
  }

  clear(): void {
    this.#giveUp?.abort();
    this.#giveUp = undefined;
    this.#drawing = undefined;
    this.box.replaceChildren();
  }
}

class HwpView implements Drawn {
  readonly #client: WorkerClient;
  readonly #pages: Page[];
  readonly #column = document.createElement('div');
  readonly #report: Report;
  readonly #follower: PageFollower;
  #zoom = 1;

  constructor(
    client: WorkerClient,
    sizes: readonly PageSize[],
    place: HTMLElement,
    report: Report,
  ) {
    this.#client = client;
    this.#report = report;
    this.#column.className = 'pages';
    this.#pages = sizes.map((size, index) => new Page(index + 1, size));
    this.#column.append(...this.#pages.map((page) => page.box));
    place.replaceChildren(this.#column);
    const observer = new IntersectionObserver(
      (entries) => {
        this.#onSight(entries);
      },
      { rootMargin: '100% 0px' },
    );
    this.#layOut();
    for (const page of this.#pages) observer.observe(page.box);
    this.#follower = new PageFollower(() => this.#pages.map((page) => page.box), report);
    let resizing: ReturnType<typeof setTimeout> | undefined;
    window.addEventListener('resize', () => {
      clearTimeout(resizing);
      resizing = setTimeout(() => {
        this.#layOut();
      }, 150);
    });
  }

  /** The scale at zoom 1: the first page as wide as the frame allows, at most its own size. */
  #scale(): number {
    const room = Math.max(1, window.innerWidth - 2 * GUTTER);
    const width = this.#pages[0]?.size.width ?? room;
    return Math.min(1, room / width) * this.#zoom;
  }

  #layOut(): void {
    const scale = this.#scale();
    this.#column.style.setProperty('--hwp-scale', String(scale));
    for (const page of this.#pages) {
      page.box.style.width = `${Math.floor(page.size.width * scale)}px`;
      page.box.style.height = `${Math.floor(page.size.height * scale)}px`;
    }
  }

  #draw(page: Page): void {
    page.draw(this.#client).catch(() => {
      // A page that cannot be drawn, or was given up as it left the view, stays blank. But a
      // worker that was stopped (too slow, or failed on its own) draws no page any more.
      const stopped = this.#client.closed;
      if (stopped instanceof SlowRenderer) this.#report.failed('slow');
      else if (stopped instanceof WorkerFailed) this.#report.failed('renderer');
    });
  }

  #onSight(entries: IntersectionObserverEntry[]): void {
    for (const entry of entries) {
      const page = this.#pages[Number((entry.target as HTMLElement).dataset.page) - 1];
      if (!page) continue;
      if (entry.isIntersecting) this.#draw(page);
      else page.clear();
    }
  }

  /** Draws the first page, the one in view when the document opens; rejects when it cannot be drawn. */
  drawFirst(): Promise<void> {
    return this.#pages[0]?.draw(this.#client) ?? Promise.resolve();
  }

  goTo(page: number): void {
    const box = this.#pages[page - 1]?.box;
    if (!box) return;
    box.scrollIntoView({ block: 'start' });
    this.#follower.pin(page);
  }

  zoom(zoom: number): void {
    const scroller = document.scrollingElement;
    const ratio = scroller ? scroller.scrollTop / Math.max(1, scroller.scrollHeight) : 0;
    this.#zoom = zoom;
    this.#layOut();
    if (scroller) scroller.scrollTop = ratio * scroller.scrollHeight;
  }
}

/** The renderer of HWP (a compound file) or HWPX (an archive). */
export function renderer(format: HwpFormat): Render {
  return async (bytes, place, report) => {
    const client = new WorkerClient(new HwpWorker(), WORK_LIMIT);
    // The viewer takes the frame out of the page when it closes: the worker stops then too.
    window.addEventListener(
      'pagehide',
      () => {
        client.close();
      },
      { once: true },
    );
    try {
      const { default: wasm } = await import('virtual:rhwp-wasm');
      const opened = await ask(client, { kind: 'open', format, bytes, wasm }, [bytes]);
      if (opened.kind !== 'opened') throw new Error('the HWP renderer answered out of turn');
      const view = new HwpView(client, opened.pages, place, report);
      // Shown once the reader sees a page: the time to draw it counts, and a first page that
      // cannot be drawn is a failure rather than a white box.
      await view.drawFirst();
      report.shown(opened.pages.length);
      return view;
    } catch (error) {
      client.close();
      throw error;
    }
  };
}
