/**
 * Draws a Word document with docx-preview into the worker's own DOM
 * (`worker-xml.ts`, `worker-html.ts`) and hands back HTML: what the Word
 * worker (`docx-worker.ts`) does with a file.
 *
 * docx-preview reads `document`, `Node`, `DOMParser` and `URL.createObjectURL`
 * from the global object; the worker puts its own there before this runs.
 * Pictures come back as blobs, each named in the HTML by a key that starts
 * with the frame's token: the frame makes their addresses (an address made
 * in the worker would end with it).
 */
import { parseAsync, renderDocument, type Options } from 'docx-preview';

import type { DocxFailure, FromDocxWorker } from './docx-protocol.ts';
import { Element, HTML_LENGTH_LIMIT, serialize, type Budget, type Node } from './worker-html.ts';
import { TooMuch } from './worker-xml.ts';

export const OPTIONS = {
  className: 'docx',
  inWrapper: true,
  breakPages: true,
  // Pages break where Word last laid the document out, as well as where it says so:
  // otherwise a document without manual breaks is one long page.
  ignoreLastRenderedPageBreak: false,
  // Fonts are read as bytes the library changes in place, so they cannot be unpacked once
  // for all; the app's fonts stand in for them.
  ignoreFonts: true,
  // Its pattern for the declaration takes time that grows with the square of a line.
  trimXmlDeclaration: false,
  renderAltChunks: false,
  renderComments: false,
  renderChanges: false,
  useBase64URL: false,
  experimental: false,
} satisfies Partial<Options>;

/** What docx-preview's package is to this: how it finds a part, and unpacks one. */
interface Package {
  get(path: string): unknown;
  load(path: string, type?: string): Promise<unknown>;
}

function packageOf(document: unknown): Package | undefined {
  if (typeof document !== 'object' || document === null || !('_package' in document)) {
    return undefined;
  }
  const found: unknown = document._package;
  if (typeof found !== 'object' || found === null) return undefined;
  return 'get' in found &&
    'load' in found &&
    typeof found.get === 'function' &&
    typeof found.load === 'function'
    ? (found as Package)
    : undefined;
}

/** The kinds of output an unpacked part is kept as: ones nothing changes after. */
const KEPT = new Set(['string', 'blob']);

/**
 * Makes the package unpack each part once for each kind of output, however
 * often the document refers to it (a picture shown in many places). A part
 * is known by the archive's own entry for it.
 */
function unpackOnce(pack: Package): void {
  const load = pack.load.bind(pack);
  const unpacked = new Map<unknown, Map<string, Promise<unknown>>>();
  pack.load = (path, type = 'string') => {
    const entry = pack.get(path);
    if (entry === undefined || !KEPT.has(type)) return load(path, type);
    const kinds = unpacked.get(entry) ?? new Map<string, Promise<unknown>>();
    unpacked.set(entry, kinds);
    const known = kinds.get(type);
    if (known) return known;
    const loading = load(path, type);
    kinds.set(type, loading);
    return loading;
  };
}

/** Why drawing stopped, from what was thrown: too much, or a file the library could not read. */
function failure(error: unknown, budget: Budget): FromDocxWorker {
  // A string or a call stack past the engine's limits is a document too large too.
  const tooMuch = error instanceof TooMuch || error instanceof RangeError || budget.passed;
  const reason: DocxFailure = tooMuch ? 'too-large' : 'unreadable';
  return { kind: 'failed', reason };
}

/** Draws the file and serializes what was drawn: the style sheets apart from the pages. */
export async function draw(
  bytes: ArrayBuffer,
  budget: Budget,
  images: () => [string, Blob][],
): Promise<FromDocxWorker> {
  let parsed: unknown;
  try {
    parsed = await parseAsync(bytes, OPTIONS);
  } catch (error) {
    return failure(error, budget);
  }
  const pack = packageOf(parsed);
  // A version of the library that keeps its package otherwise fails here (as the renderer's
  // failure) rather than draws without the guard.
  if (!pack) return { kind: 'failed', reason: 'renderer' };
  unpackOnce(pack);
  try {
    const nodes = (await renderDocument(parsed, OPTIONS)) as unknown as Node[];
    // A ceiling passed where the library caught what was thrown.
    if (budget.passed) throw new TooMuch('elements');
    const isStyle = (node: Node): boolean => node instanceof Element && node.tagName === 'style';
    const styles = serialize(nodes.filter(isStyle));
    const body = serialize(
      nodes.filter((node) => !isStyle(node)),
      HTML_LENGTH_LIMIT - styles.length,
    );
    return { kind: 'drawn', styles, body, images: images() };
  } catch (error) {
    return failure(error, budget);
  }
}
