/**
 * The Word worker's globals (`src/viewer-frame/docx-worker.ts`), put on
 * Node's global object for a test and taken off again: the same DOM, the
 * same ceilings, pictures kept by key.
 */
import { draw } from '../../src/viewer-frame/docx-draw.ts';
import type { FromDocxWorker } from '../../src/viewer-frame/docx-protocol.ts';
import { Budget, documentFor, Node } from '../../src/viewer-frame/worker-html.ts';
import { domParserFor, NodeCount } from '../../src/viewer-frame/worker-xml.ts';

/** Draws a Word file as the worker does, with `token` before each picture's key. */
export async function drawAsWorker(file: Buffer, token = 'TOKEN'): Promise<FromDocxWorker> {
  const names = ['document', 'Node', 'DOMParser', 'requestAnimationFrame'] as const;
  // As they were, accessors included (jsdom's `document` has only a getter).
  const before = names.map((name) => Object.getOwnPropertyDescriptor(globalThis, name));
  const put = (name: string, value: unknown): void => {
    Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
  };
  const createObjectURL = URL.createObjectURL.bind(URL);
  const budget = new Budget();
  const images: [string, Blob][] = [];
  put('document', documentFor(budget));
  put('Node', Node);
  put('DOMParser', domParserFor(new NodeCount()));
  put('requestAnimationFrame', () => 0);
  URL.createObjectURL = (blob: Blob | MediaSource) => {
    const key = `${token}-${images.length}`;
    if (blob instanceof Blob) images.push([key, blob]);
    return key;
  };
  try {
    return await draw(Uint8Array.from(file).buffer, budget, () => images);
  } finally {
    names.forEach((name, at) => {
      const descriptor = before[at];
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    });
    URL.createObjectURL = createObjectURL;
  }
}
