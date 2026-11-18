/**
 * What the HWP renderer's worker does (hwp-worker.ts), apart from the
 * worker so that it can be tried in Node: checks a file, opens it with rhwp,
 * and draws its pages one at a time.
 *
 * Before rhwp reads a file it is checked: an archive (HWPX) is unpacked,
 * counted and written anew (zip-guard.ts). rhwp lays the
 * document out with font metrics of its own (it has no `document` in a
 * worker to measure text with), so the layout does not depend on the fonts
 * the reader has; only the glyphs drawn do.
 */
import init, { HwpDocument } from '@rhwp/core';

import { PAGE_LIMIT } from '$lib/file-viewer/protocol.ts';

import type { FromWorker, PageSize, TextRun, ToWorker } from './hwp-messages.ts';
import { LockedFile, UnreadableFile } from './renderer.ts';
import { repack } from './zip-guard.ts';

/** What rhwp says of a file it cannot read, and of one locked with a password (0.8.7). */
const INVALID_FILE = '유효하지 않은 파일';
const NEEDS_PASSWORD = '비밀번호가 필요한';

/** The value of a lowercase hexadecimal digit, from its character code. */
const digit = (code: number): number => (code <= 0x39 ? code - 0x30 : code - 0x57);

/** The bytes of lowercase hexadecimal text, as the build writes rhwp's WebAssembly. */
export function fromHex(text: string): Uint8Array<ArrayBuffer> {
  const native = (Uint8Array as unknown as { fromHex?: (hex: string) => Uint8Array<ArrayBuffer> })
    .fromHex;
  if (native) return native(text);
  const bytes = new Uint8Array(text.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = (digit(text.charCodeAt(2 * index)) << 4) | digit(text.charCodeAt(2 * index + 1));
  }
  return bytes;
}

/** Why rhwp did not open a file, from what it says. */
function openFailure(error: unknown): Error {
  const said = error instanceof Error ? error.message : String(error);
  if (said.includes(NEEDS_PASSWORD)) return new LockedFile();
  return said.startsWith(INVALID_FILE) ? new UnreadableFile() : new Error(said);
}

const isSize = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value > 0;

function pageSize(document: HwpDocument, index: number): PageSize {
  const info: unknown = JSON.parse(document.getPageInfo(index));
  const { width, height } = (info ?? {}) as { width?: unknown; height?: unknown };
  if (!isSize(width) || !isSize(height)) throw new UnreadableFile();
  return { width, height };
}

/** A page's runs of text, as rhwp lays them out (`getPageTextLayout`). */
function textOf(document: HwpDocument, index: number): TextRun[] {
  const layout: unknown = JSON.parse(document.getPageTextLayout(index));
  const runs = (layout as { runs?: unknown } | null)?.runs;
  if (!Array.isArray(runs)) return [];
  return runs.flatMap((run: Record<string, unknown>) => {
    const { text, x, y, w, h, fontSize } = run;
    const numbers = [x, y, w, h, fontSize];
    if (
      typeof text !== 'string' ||
      !numbers.every((n) => typeof n === 'number' && Number.isFinite(n))
    ) {
      return [];
    }
    const [left, top, width, height, size] = numbers as number[];
    return [{ text, x: left, y: top, width, height, size }] as TextRun[];
  });
}

/** The worker's document: opened once, then drawn a page at a time. */
export class HwpReader {
  #document: HwpDocument | undefined;

  async open(message: Extract<ToWorker, { kind: 'open' }>): Promise<FromWorker> {
    if (this.#document) throw new Error('a document is open already');
    const checked = await repack(message.bytes);
    await init({ module_or_path: fromHex(message.wasm) });
    let document: HwpDocument;
    try {
      document = new HwpDocument(new Uint8Array(checked));
    } catch (error) {
      throw openFailure(error);
    }
    this.#document = document;
    const count = document.pageCount();
    if (!Number.isInteger(count) || count < 1 || count > PAGE_LIMIT) throw new UnreadableFile();
    const pages = Array.from({ length: count }, (_, index) => pageSize(document, index));
    return { kind: 'opened', pages };
  }

  /** A page (from 1), drawn. */
  page(number: number): FromWorker {
    const document = this.#document;
    if (!document || !Number.isInteger(number) || number < 1 || number > document.pageCount()) {
      throw new Error('no such page');
    }
    const index = number - 1;
    return { kind: 'page', svg: document.renderPageSvg(index), text: textOf(document, index) };
  }

  /** The answer to a message: what was asked, or why it could not be done. */
  async answer(message: ToWorker): Promise<FromWorker> {
    try {
      return message.kind === 'open' ? await this.open(message) : this.page(message.page);
    } catch (error) {
      return { kind: 'failed', reason: failureOf(error) };
    }
  }
}

function failureOf(error: unknown): Extract<FromWorker, { kind: 'failed' }>['reason'] {
  if (error instanceof LockedFile) return 'locked';
  return error instanceof UnreadableFile ? 'unreadable' : 'renderer';
}
