/**
 * The kinds of file the viewer draws, told apart by their first bytes.
 *
 * A file's name and its declared type are what the person who uploaded it
 * said; they choose an icon and nothing else. Whether the viewer opens a file,
 * and with which renderer, is decided by what the bytes are. A file whose
 * bytes match no kind here is offered as a download.
 *
 * Adding a kind: a sniffer here and a renderer in the frame
 * (`src/viewer-frame/renderers.ts`, whose table is typed by `FormatId` and
 * so asks for it).
 */
import { zipMemberNames } from './zip-names.ts';

/** The largest file the viewer opens; a larger one is only offered as a download. */
export const VIEW_LIMIT_BYTES = 50 * 1024 * 1024;

/** How far into a PDF its header may lie: readers accept up to 1024 bytes of anything before it. */
const PDF_HEADER_WINDOW = 1024;
const PDF_HEADER = [0x25, 0x50, 0x44, 0x46, 0x2d]; // "%PDF-"

function startsAt(bytes: Uint8Array, at: number, expected: readonly number[]): boolean {
  return expected.every((byte, index) => bytes[at + index] === byte);
}

function isPdf(bytes: Uint8Array): boolean {
  const last = Math.min(PDF_HEADER_WINDOW, bytes.length - PDF_HEADER.length);
  for (let at = 0; at <= last; at += 1) {
    if (startsAt(bytes, at, PDF_HEADER)) return true;
  }
  return false;
}

/** An Office Open XML package whose main part is `member`: a ZIP archive that has it. */
const officePackage =
  (member: string) =>
  (bytes: Uint8Array): boolean =>
    zipMemberNames(bytes)?.includes(member) ?? false;

/** Every kind, in the order they are tried. */
const SNIFFERS = {
  pdf: isPdf,
  docx: officePackage('word/document.xml'),
} as const satisfies Record<string, (bytes: Uint8Array) => boolean>;

export type FormatId = keyof typeof SNIFFERS;

export const FORMAT_IDS = Object.keys(SNIFFERS) as FormatId[];

/** The kind of file the bytes are, or undefined when the viewer does not draw it. */
export function sniffFormat(bytes: Uint8Array): FormatId | undefined {
  return FORMAT_IDS.find((id) => SNIFFERS[id](bytes));
}

export function isFormatId(value: unknown): value is FormatId {
  return typeof value === 'string' && (FORMAT_IDS as string[]).includes(value);
}
