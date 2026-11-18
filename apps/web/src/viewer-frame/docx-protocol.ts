/**
 * The messages between the viewer's frame and its Word worker
 * (`docx-worker.ts`): the frame hands over the file's bytes and a token, the
 * worker answers once, with the drawn document or why it could not be drawn.
 * The frame reads an answer only in these shapes.
 */

/** Why the worker could not draw the file. */
export type DocxFailure = 'unreadable' | 'too-large' | 'renderer';

export interface ToDocxWorker {
  bytes: ArrayBuffer;
  /** What each picture's key in the HTML starts with: unknown to the document. */
  token: string;
}

export type FromDocxWorker =
  | {
      kind: 'drawn';
      /** The document's style sheets, as `<style>` elements. */
      styles: string;
      /** The pages. */
      body: string;
      /** Each picture, by its key in the HTML. */
      images: [string, Blob][];
    }
  | { kind: 'failed'; reason: DocxFailure };

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null;

/** The worker's answer, or undefined when it is not in one of the shapes. */
export function readFromDocxWorker(data: unknown): FromDocxWorker | undefined {
  if (!isRecord(data)) return undefined;
  if (data.kind === 'failed') {
    const reason = data.reason;
    return reason === 'unreadable' || reason === 'too-large' || reason === 'renderer'
      ? { kind: 'failed', reason }
      : undefined;
  }
  if (data.kind !== 'drawn' || typeof data.styles !== 'string' || typeof data.body !== 'string') {
    return undefined;
  }
  if (!Array.isArray(data.images)) return undefined;
  const images: [string, Blob][] = [];
  for (const image of data.images as unknown[]) {
    if (!Array.isArray(image) || typeof image[0] !== 'string' || !(image[1] instanceof Blob)) {
      return undefined;
    }
    images.push([image[0], image[1]]);
  }
  return { kind: 'drawn', styles: data.styles, body: data.body, images };
}
