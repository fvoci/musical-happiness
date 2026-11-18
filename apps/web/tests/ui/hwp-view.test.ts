import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { renderer } from '../../src/viewer-frame/hwp.ts';
import { HwpView } from '../../src/viewer-frame/hwp-view.ts';
import type { Report } from '../../src/viewer-frame/renderer.ts';
import {
  WorkerClient,
  WorkerClosed,
  type WorkerLike,
} from '../../src/viewer-frame/worker-client.ts';

/** A worker that never answers unless told to, and records what it was sent. */
class FakeWorker implements WorkerLike {
  readonly #events = new EventTarget();
  sent: unknown[] = [];
  terminated = false;

  addEventListener(type: string, listener: (event: never) => void): void {
    this.#events.addEventListener(type, listener as EventListener);
  }

  postMessage(message: unknown): void {
    this.sent.push(message);
  }

  terminate(): void {
    this.terminated = true;
  }

  answer(data: unknown): void {
    this.#events.dispatchEvent(new MessageEvent('message', { data }));
  }
}

/** The observers made, so that a test can say what came into view. */
const observers: { callback: IntersectionObserverCallback }[] = [];

class FakeObserver {
  constructor(callback: IntersectionObserverCallback) {
    observers.push({ callback });
  }
  readonly observe = vi.fn();
  readonly disconnect = vi.fn();
}

const report = () =>
  ({
    shown: vi.fn<Report['shown']>(),
    page: vi.fn<Report['page']>(),
    failed: vi.fn<Report['failed']>(),
  }) satisfies Report;

describe('the pages of an HWP document, once shown', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('IntersectionObserver', FakeObserver);
    observers.length = 0;
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('a page the worker takes too long over stops it, and the viewer is told', async () => {
    const worker = new FakeWorker();
    const client = new WorkerClient(worker, 1000);
    const told = report();
    const place = document.createElement('main');
    new HwpView(
      client,
      [
        { width: 800, height: 1100 },
        { width: 800, height: 1100 },
      ],
      place,
      told,
    );
    const second = place.querySelector('[data-page="2"]');
    if (!second) throw new Error('no box for page 2');
    observers[0]?.callback(
      [{ target: second, isIntersecting: true } as unknown as IntersectionObserverEntry],
      {} as IntersectionObserver,
    );
    expect(worker.sent).toEqual([{ kind: 'page', page: 2 }]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(worker.terminated).toBe(true);
    expect(told.failed).toHaveBeenCalledWith('slow');
  });

  it('a page past what the frame takes on: the file is too large, and the worker is stopped', async () => {
    const worker = new FakeWorker();
    const client = new WorkerClient(worker, 1000);
    const told = report();
    const place = document.createElement('main');
    new HwpView(
      client,
      [
        { width: 800, height: 1100 },
        { width: 800, height: 1100 },
      ],
      place,
      told,
    );
    const second = place.querySelector('[data-page="2"]');
    if (!second) throw new Error('no box for page 2');
    observers[0]?.callback(
      [{ target: second, isIntersecting: true } as unknown as IntersectionObserverEntry],
      {} as IntersectionObserver,
    );
    worker.answer({ kind: 'failed', reason: 'too-large' });
    await vi.advanceTimersByTimeAsync(10);
    expect(told.failed).toHaveBeenCalledWith('too-large');
    expect(worker.terminated).toBe(true);
  });

  it('a page that left the view before it was drawn is not reported', async () => {
    const worker = new FakeWorker();
    const client = new WorkerClient(worker, 1000);
    const told = report();
    const place = document.createElement('main');
    new HwpView(
      client,
      [
        { width: 800, height: 1100 },
        { width: 800, height: 1100 },
      ],
      place,
      told,
    );
    const [first, second] = [...place.querySelectorAll('.page')];
    const sight = (target: Element | undefined, isIntersecting: boolean): void => {
      observers[0]?.callback(
        [{ target, isIntersecting } as unknown as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    };
    sight(first, true);
    sight(second, true);
    sight(second, false);
    worker.answer({ kind: 'page', svg: '<svg xmlns="http://www.w3.org/2000/svg"/>', text: [] });
    await vi.advanceTimersByTimeAsync(10);
    // Page 2 was given up before it was sent: only page 1 went to the worker.
    expect(worker.sent).toEqual([{ kind: 'page', page: 1 }]);
    expect(first?.querySelector('svg')).not.toBeNull();
    expect(told.failed).not.toHaveBeenCalled();
  });
});

describe('the renderer of HWP documents', () => {
  it('stops its worker when the frame goes (pagehide), even while it opens a file', async () => {
    const worker = new FakeWorker();
    const render = renderer('hwpx', {
      worker: () => worker,
      wasm: async () => '00',
      limit: 30_000,
    });
    const opening = render(new ArrayBuffer(4), document.createElement('main'), report());
    await vi.waitFor(() => {
      expect(worker.sent).toHaveLength(1);
    });
    window.dispatchEvent(new PageTransitionEvent('pagehide'));
    expect(worker.terminated).toBe(true);
    await expect(opening).rejects.toBeInstanceOf(WorkerClosed);
  });
});
