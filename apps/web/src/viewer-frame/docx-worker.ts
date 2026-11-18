/**
 * The Word worker: docx-preview reads and draws a Word document here, off
 * the frame's main thread, so that the viewer's time limit and its "Close"
 * hold while it works. The frame ends the worker when it has answered, and
 * the viewer throws the whole frame away (and the worker with it) when the
 * time is up or the viewer closes.
 *
 * A worker has no `document` and no `DOMParser`; this puts the worker's own
 * on the global object (`worker-html.ts`, `worker-xml.ts`), counting every
 * node against ceilings, and keeps the pictures as blobs for the frame. Like
 * pdf.js's worker, it is built into the frame's chunk and started from a
 * `blob:` (`?worker&inline`).
 */
import { draw } from './docx-draw.ts';
import type { ToDocxWorker } from './docx-protocol.ts';
import { Budget, documentFor, Node } from './worker-html.ts';
import { domParserFor, NodeCount } from './worker-xml.ts';

const scope = globalThis as unknown as Record<string, unknown> & {
  postMessage(message: unknown): void;
};

scope.onmessage = (event: MessageEvent<ToDocxWorker>) => {
  const { bytes, token } = event.data;
  const budget = new Budget();
  const images: [string, Blob][] = [];
  scope.document = documentFor(budget);
  scope.Node = Node;
  scope.DOMParser = domParserFor(new NodeCount());
  // Only shapes ask for a frame, to measure themselves: there is no layout here.
  scope.requestAnimationFrame = () => 0;
  URL.createObjectURL = (blob: Blob | MediaSource) => {
    const key = `${token}-${images.length}`;
    if (blob instanceof Blob) images.push([key, blob]);
    return key;
  };
  void draw(bytes, budget, () => images).then((answer) => {
    scope.postMessage(answer);
  });
};
