/**
 * What changed from one stored body to another (logic/body-diff.ts), found
 * in a worker (diff-worker.ts) so that the page goes on while the largest
 * bodies are compared. Where there are no workers (a test's simulated page,
 * or one that failed to start) they are compared here.
 *
 * Both bodies have to be ones the validator accepts: what comes back is drawn
 * by `drawDiffBlocks` (render-diff.ts), which does not check them again.
 * The comparison itself is in the worker's chunk and, for here, one loaded on
 * first use: the page's first chunk does not hold it.
 */
import type { BodyDiff, BodyDiffOptions } from '$lib/logic/body-diff.ts';

import { DIAGRAM_LANGUAGE, type DocNode } from './schema.ts';

const OPTIONS: BodyDiffOptions = { diagramLanguage: DIAGRAM_LANGUAGE };

let worker: Promise<Worker | undefined> | undefined;
const waiting = new Map<number, (diff: BodyDiff | undefined) => void>();
let last = 0;

function start(): Promise<Worker | undefined> {
  worker ??= import('./diff-worker.ts?worker').then(
    ({ default: DiffWorker }) => {
      let started: Worker;
      try {
        started = new DiffWorker();
      } catch {
        return undefined;
      }
      started.addEventListener('message', (event: MessageEvent<{ id: number; diff: BodyDiff }>) => {
        const answer = waiting.get(event.data.id);
        waiting.delete(event.data.id);
        answer?.(event.data.diff);
      });
      // A worker that fails takes what it was asked with it: that is compared here instead.
      started.addEventListener('error', () => {
        started.terminate();
        worker = Promise.resolve(undefined);
        for (const answer of waiting.values()) answer(undefined);
        waiting.clear();
      });
      return started;
    },
    () => undefined,
  );
  return worker;
}

/** What changed from `before` to `after`, two bodies the validator accepts (`null` for none). */
export async function compareBodies(
  before: DocNode | null,
  after: DocNode | null,
): Promise<BodyDiff> {
  const running = await start();
  if (running) {
    const id = (last += 1);
    const diff = await new Promise<BodyDiff | undefined>((answer) => {
      waiting.set(id, answer);
      running.postMessage({ id, before, after, options: OPTIONS });
    });
    if (diff) return diff;
  }
  // Loaded only now: the comparison is no part of the page until a version is compared.
  const { diffBodies } = await import('$lib/logic/body-diff.ts');
  return diffBodies(before, after, OPTIONS);
}
