/**
 * The kinds of file the viewer draws, told apart by their first bytes.
 *
 * A file's name and its declared type are what the person who uploaded it
 * said; they choose an icon and nothing else. Whether the viewer opens a file,
 * and with which renderer, is decided by what the bytes are. A file whose
 * bytes match no kind here is offered as a download.
 *
 * A kind that is a ZIP archive (the Office formats, HWPX) is told apart by
 * its parts (`parts` in `FORMATS`, never `bytes`), and is then held to limits
 * on what they unpack to, read from the archive's directory before anything
 * is unpacked: a small file can unpack to more than a browser tab survives.
 * What the directory says is checked in the frame, which unpacks the parts,
 * counts them and hands its renderer a new archive of only what it counted
 * (src/viewer-frame/zip-guard.ts).
 *
 * Adding a kind: its line in `FORMATS` here, its extensions in
 * `extensions.ts` and a renderer in the frame (`src/viewer-frame/renderers.ts`);
 * both tables are typed by `FormatId` and so ask for them.
 */
import { zipEntries, type ZipEntry } from './zip-directory.ts';

/** The largest file the viewer opens, kept where the first screen reads it (`extensions.ts`). */
export { VIEW_LIMIT_BYTES } from './extensions.ts';

/** The most an archive's parts may unpack to, together. */
export const UNPACKED_LIMIT_BYTES = 200 * 1024 * 1024;
/**
 * The most its XML parts (`.xml`, `.rels`) may unpack to, together: the
 * renderers parse them into documents in memory, many times their size.
 */
export const MARKUP_LIMIT_BYTES = 30 * 1024 * 1024;

const MARKUP = /\.(?:xml|rels)$/i;

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

/** How a kind is told apart. */
type Kind =
  /** By its bytes. */
  | { bytes: (bytes: Uint8Array) => boolean }
  /**
   * By the parts of the ZIP archive it is. Such a kind is held to the limits
   * on what its parts unpack to, and the frame watches it unpack.
   */
  | { parts: (entries: readonly ZipEntry[]) => boolean };

/** An Office Open XML package whose main part is `member`: a ZIP archive that has it. */
const officePackage = (member: string): Kind => ({
  parts: (entries) => entries.some((entry) => entry.name === member),
});

/** Every kind, in the order they are tried. */
const FORMATS = {
  pdf: { bytes: isPdf },
  docx: officePackage('word/document.xml'),
  xlsx: officePackage('xl/workbook.xml'),
  pptx: officePackage('ppt/presentation.xml'),
} as const satisfies Record<string, Kind>;

export type FormatId = keyof typeof FORMATS;

export const FORMAT_IDS = Object.keys(FORMATS) as FormatId[];

/** Whether the kind is a ZIP archive, held to the limits and watched as it unpacks. */
export function isPackaged(format: FormatId): boolean {
  return 'parts' in FORMATS[format];
}

/** What the viewer makes of a file's bytes. */
export type Examined =
  | { kind: 'format'; format: FormatId }
  /** A kind the viewer draws, but an archive that unpacks past the limits. */
  | { kind: 'too-large'; format: FormatId }
  | { kind: 'unknown' };

/** Whether an archive's parts say they unpack to more than the viewer takes. */
export function unpacksTooLarge(entries: readonly ZipEntry[]): boolean {
  let total = 0;
  let markup = 0;
  for (const entry of entries) {
    total += entry.size;
    if (MARKUP.test(entry.name)) markup += entry.size;
  }
  return total > UNPACKED_LIMIT_BYTES || markup > MARKUP_LIMIT_BYTES;
}

/** The kind of file the bytes are, and whether the viewer takes them. */
export function examine(bytes: Uint8Array): Examined {
  const entries = zipEntries(bytes);
  const format = FORMAT_IDS.find((id) => {
    const kind = FORMATS[id];
    return 'bytes' in kind ? kind.bytes(bytes) : entries !== undefined && kind.parts(entries);
  });
  if (!format) return { kind: 'unknown' };
  if (entries && isPackaged(format) && unpacksTooLarge(entries)) {
    return { kind: 'too-large', format };
  }
  return { kind: 'format', format };
}

export function isFormatId(value: unknown): value is FormatId {
  return typeof value === 'string' && (FORMAT_IDS as string[]).includes(value);
}
