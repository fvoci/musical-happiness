/**
 * What rhwp parses of an HWPX package, held to the viewer's limit on XML, in
 * the renderer's worker once the archive was counted and written anew
 * (zip-guard.ts).
 *
 * zip-guard counts a part as XML by what it holds (its first character "<"),
 * and each part once, as the Word renderer reads them. rhwp (0.8.7) reads an
 * HWPX otherwise, from the package's own list (`Contents/content.hpf`):
 *
 * - the sections are the parts the spine names (or, when it names none, every
 *   XML part of the manifest whose address says "section"), in that order,
 *   each parsed as XML whatever it starts with, as often as it is named;
 * - the master pages (the manifest's XML parts whose address says
 *   "masterpage") are parsed again for each section that uses them.
 *
 * So here a spine that names one part twice stops the file (no document does
 * that), every part counts as XML but the pictures and other data under
 * `BinData/`, and the master pages count once more for every section after
 * the first. Together they are held to `MARKUP_LIMIT_BYTES`.
 *
 * The package is read as rhwp's reader reads it: elements by their local
 * name (`item`, `itemref`), attributes by their exact name (`id`, `href`,
 * `media-type`, `idref`), comments and character data left out.
 */
import { MARKUP_LIMIT_BYTES } from '$lib/file-viewer/formats.ts';
import { zipEntries, type ZipEntry } from '$lib/file-viewer/zip-directory.ts';

import { TooLargeFile, UnreadableFile } from './renderer.ts';

const PACKAGE = 'Contents/content.hpf';
const XML = 'application/xml';
const LOCAL_SIZE = 30;
const STORED = 0;

interface Item {
  id: string;
  href: string;
  mediaType: string;
}

/** What the package lists: its items, and the ids its spine names in order. */
interface Listed {
  items: Item[];
  spine: string[];
}

const ENTITIES: Record<string, string> = { lt: '<', gt: '>', amp: '&', quot: '"', apos: "'" };

/** An attribute's value with XML's entities and character references read. */
function unescape(value: string): string {
  return value.replace(/&(#x[0-9a-f]+|#\d+|\w+);/gi, (whole, name: string) => {
    if (name.startsWith('#x') || name.startsWith('#X')) {
      return String.fromCodePoint(Number.parseInt(name.slice(2), 16));
    }
    if (name.startsWith('#')) return String.fromCodePoint(Number.parseInt(name.slice(1), 10));
    return ENTITIES[name] ?? whole;
  });
}

function attributes(text: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const match of text.matchAll(/([^\s=/>]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) {
    const [, name = '', double, single] = match;
    if (!found.has(name)) found.set(name, unescape(double ?? single ?? ''));
  }
  return found;
}

/** The items and the spine of `content.hpf`'s text. */
export function listed(xml: string): Listed {
  const text = xml.replace(/<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<\?[\s\S]*?\?>/g, '');
  const items: Item[] = [];
  const spine: string[] = [];
  const tags = /<(?:[\w.-]+:)?(item|itemref)(?=[\s/>])((?:[^>"']|"[^"]*"|'[^']*')*)>/g;
  for (const [, name, rest = ''] of text.matchAll(tags)) {
    const found = attributes(rest);
    if (name === 'itemref') {
      const idref = found.get('idref');
      if (idref !== undefined) spine.push(idref);
      continue;
    }
    const id = found.get('id') ?? '';
    const href = found.get('href') ?? '';
    if (id && href) items.push({ id, href, mediaType: found.get('media-type') ?? '' });
  }
  return { items, spine };
}

/** The parts rhwp reads as sections, in order, as many times as it reads each. */
export function sectionsOf({ items, spine }: Listed): string[] {
  const isSection = (item: Item): boolean =>
    item.mediaType === XML && item.href.includes('section');
  const named = spine.flatMap((idref) => {
    const item = items.find((each) => each.id === idref);
    return item && isSection(item) ? [item.href] : [];
  });
  if (named.length > 0) return named;
  return items
    .filter(isSection)
    .map((item) => item.href)
    .sort();
}

/** The master pages rhwp may read for each section: each address once. */
export function masterPagesOf({ items }: Listed): string[] {
  const pages = items.filter(
    (item) => item.mediaType === XML && item.href.toLowerCase().includes('masterpage'),
  );
  return [...new Set(pages.map((item) => item.href))];
}

const decoder = new TextDecoder();

/** The text of a part of an archive that zip-guard wrote (honest offsets and sizes). */
async function partText(bytes: Uint8Array, entry: ZipEntry): Promise<string> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const at = entry.offset;
  const start = at + LOCAL_SIZE + view.getUint16(at + 26, true) + view.getUint16(at + 28, true);
  const packed = bytes.slice(start, start + entry.compressedSize);
  if (entry.method === STORED) return decoder.decode(packed);
  const stream = new Blob([packed]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  return decoder.decode(await new Response(stream).arrayBuffer());
}

/**
 * Throws `TooLargeFile` when what rhwp would parse of the package as XML is
 * past the limit, and `UnreadableFile` when its spine names a part twice.
 */
export async function checkHwpxPackage(archive: ArrayBuffer): Promise<void> {
  const bytes = new Uint8Array(archive);
  const entries = zipEntries(bytes);
  if (!entries) throw new UnreadableFile();
  const main = entries.find((entry) => entry.name === PACKAGE);
  const list = main ? listed(await partText(bytes, main)) : { items: [], spine: [] };
  const sections = sectionsOf(list);
  if (new Set(sections).size !== sections.length) throw new UnreadableFile();
  const masterPages = new Set(masterPagesOf(list));
  const parsed = new Set([...sections, ...masterPages]);
  let total = 0;
  for (const entry of entries) {
    if (entry.name.startsWith('BinData/') && !parsed.has(entry.name)) continue;
    const times = masterPages.has(entry.name) ? Math.max(1, sections.length) : 1;
    total += times * entry.size;
    if (total > MARKUP_LIMIT_BYTES) throw new TooLargeFile();
  }
}
