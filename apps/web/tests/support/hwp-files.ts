/**
 * Files for the tests of the HWP viewer, made here rather than kept as
 * binaries: an HWP and an HWPX document in Korean with a header, a table, a
 * picture and a second page, written by rhwp's own exporter (`@rhwp/core`,
 * the viewer's renderer, run in Node), and a writer of compound files (OLE)
 * that can tell the lies the viewer must catch. Every file is ours, so the
 * tests carry no third-party content.
 */
import { inflateRawSync } from 'node:zlib';

import { HwpDocument, initSync } from '@rhwp/core';

import {
  childrenOf,
  COMPOUND_TYPES,
  compoundFile as readCompound,
  readStream,
  type CompoundEntry,
} from '../../src/lib/file-viewer/compound-file.ts';
import { zipEntries } from '../../src/lib/file-viewer/zip-directory.ts';

import { rhwpWasmHex } from '../../tools/build/rhwp-wasm.ts';

import { png, zip, type ZipMember } from './viewer-files.ts';

/** The text of the sample, for the tests to look for. */
export const HWP_TEXT = {
  header: '머리글: 회의록',
  title: '분기 회의록',
  body: '이 문서는 한국어 견본입니다. 표와 그림이 들어 있습니다.',
  cells: ['항목', '담당', '기한', '설계 검토', '김민지', '11월 20일'],
  second: '둘째 쪽: 다음 회의 안건',
};

/** The password of the locked samples. */
export const HWP_PASSWORD = '비밀1234';

let ready = false;

/**
 * Starts rhwp from the WebAssembly the build hands the frame, its memory held
 * to the same limit: rhwp's loader keeps the first instance it is given, so the
 * worker's reader, tried in the same module context, runs on this one.
 */
function startRhwp(): void {
  if (ready) return;
  initSync({ module: Buffer.from(rhwpWasmHex(), 'hex') });
  ready = true;
}

function done(result: string): void {
  const answer = JSON.parse(result) as { ok?: boolean };
  if (!answer.ok) throw new Error(`rhwp: ${result}`);
}

/** HWPUNITs (1/7200 inch) per CSS pixel. */
const UNITS_PER_PIXEL = 75;

/** The sample, two pages, as rhwp holds it. */
function koreanDocument(): HwpDocument {
  startRhwp();
  const document = HwpDocument.createEmpty();
  document.createBlankDocument();
  // A paragraph per line: the title, the body, the table's, the picture's, the second page's.
  const lines = [HWP_TEXT.title, HWP_TEXT.body, '', '', HWP_TEXT.second];
  lines.forEach((line, index) => {
    if (line) done(document.insertText(0, index, 0, line));
    if (index < lines.length - 1) done(document.splitParagraph(0, index, line.length));
  });
  done(document.createTable(0, 2, 0, 2, 3));
  HWP_TEXT.cells.forEach((cell, index) => {
    done(document.insertTextInCell(0, 2, 0, index, 0, 0, cell));
  });
  // Placed below the table, from the paper's corner: without an offset rhwp puts it in the corner.
  const [x, y] = [113 * UNITS_PER_PIXEL, 240 * UNITS_PER_PIXEL];
  const picture = png(200, 100, [46, 125, 210]);
  done(document.insertPicture(0, 3, 0, '', picture, 15000, 7500, 200, 100, 'png', '그림', x, y));
  done(document.createHeaderFooter(0, true, 0));
  done(document.insertTextInHeaderFooter(0, true, 0, 0, 0, HWP_TEXT.header));
  done(document.insertPageBreak(0, 4, 0));
  return document;
}

/**
 * The part rhwp adds to an HWPX it writes from its own document, which makes
 * it read the file back with the layout rules of HWP. Hancom's editor drops it
 * on saving; the samples drop it too, to be read as any HWPX is.
 */
const RHWP_ORIGIN = 'META-INF/rhwp-hwp5-origin';

/**
 * The archive again with each part as `change` makes it (the same, another,
 * or left out when it gives undefined), each stored or deflated as it was.
 */
export function rewriteArchive(
  archive: Buffer,
  change: (member: ZipMember) => ZipMember | undefined,
): Buffer {
  const members = (zipEntries(archive) ?? []).flatMap((entry) => {
    const at = entry.offset;
    const start = at + 30 + archive.readUInt16LE(at + 26) + archive.readUInt16LE(at + 28);
    const packed = archive.subarray(start, start + entry.compressedSize);
    const stored = entry.method === 0;
    const changed = change({
      name: entry.name,
      data: stored ? packed : inflateRawSync(packed),
      stored,
    });
    return changed ? [changed] : [];
  });
  return zip(members);
}

export function koreanHwpx(): Buffer {
  const exported = Buffer.from(koreanDocument().exportHwpx());
  return rewriteArchive(exported, (member) => (member.name === RHWP_ORIGIN ? undefined : member));
}

export function koreanHwp(): Buffer {
  return Buffer.from(koreanDocument().exportHwp());
}

export function lockedHwp(): Buffer {
  return Buffer.from(koreanDocument().exportHwpWithPassword(HWP_PASSWORD));
}

export function lockedHwpx(): Buffer {
  return Buffer.from(koreanDocument().exportHwpxWithPassword(HWP_PASSWORD));
}

const MINI_SECTOR = 64;
const MINI_CUTOFF = 4096;
const END = 0xfffffffe;
const FREE = 0xffffffff;
const FAT_SECTOR = 0xfffffffd;
const DIFAT_SECTOR = 0xfffffffc;
/** The FAT sectors the header lists; DIFAT sectors list the rest. */
const HEADER_FAT_SECTORS = 109;
const NONE = 0xffffffff;

/** A stream of a compound file, and the lies the file can tell about it. */
export interface CompoundStream {
  /** Its path, with "/" between storages: `BodyText/Section0`. */
  path: string;
  data: Buffer;
  /** The size its directory entry gives, when not its own. */
  said?: number;
  /** Its entry points at the first sector of this stream's chain instead of its own. */
  sharesWith?: string;
}

interface Node {
  name: string;
  type: 1 | 2 | 5;
  children: Map<string, Node>;
  stream?: CompoundStream;
  id: number;
  start: number;
  size: number;
  child: number;
  right: number;
}

/** Names in the order the format sorts siblings: shorter first, then by upper case. */
function compare(a: string, b: string): number {
  if (a.length !== b.length) return a.length - b.length;
  const [upperA, upperB] = [a.toUpperCase(), b.toUpperCase()];
  return upperA < upperB ? -1 : upperA > upperB ? 1 : 0;
}

function tree(streams: readonly CompoundStream[]): Node[] {
  const node = (name: string, type: Node['type']): Node => ({
    name,
    type,
    children: new Map(),
    id: 0,
    start: END,
    size: 0,
    child: NONE,
    right: NONE,
  });
  const root = node('Root Entry', 5);
  for (const stream of streams) {
    const parts = stream.path.split('/');
    let at = root;
    parts.forEach((part, index) => {
      const leaf = index === parts.length - 1;
      const next = at.children.get(part) ?? node(part, leaf ? 2 : 1);
      if (leaf) next.stream = stream;
      at.children.set(part, next);
      at = next;
    });
  }
  const nodes: Node[] = [];
  const visit = (each: Node): void => {
    each.id = nodes.length;
    nodes.push(each);
    const children = [...each.children.values()].sort((a, b) => compare(a.name, b.name));
    children.forEach(visit);
    // A right-leaning chain of siblings in order: a valid (if unbalanced) tree.
    each.child = children[0]?.id ?? NONE;
    children.forEach((child, index) => {
      child.right = children[index + 1]?.id ?? NONE;
    });
  };
  visit(root);
  return nodes;
}

/** Sectors of `size` holding `data`, and their chain in `table`; the first sector, or END. */
function place(sectors: Buffer[], table: number[], data: Buffer, size: number): number {
  const count = Math.ceil(data.length / size);
  if (count === 0) return END;
  const first = sectors.length;
  for (let index = 0; index < count; index += 1) {
    const sector = Buffer.alloc(size);
    data.copy(sector, 0, index * size, (index + 1) * size);
    sectors.push(sector);
    table.push(index === count - 1 ? END : first + index + 1);
  }
  return first;
}

/** The directory, in whole sectors of `size`; the entries past the nodes unused. */
function directory(nodes: readonly Node[], size: number): Buffer {
  const perSector = size / 128;
  const out = Buffer.alloc(Math.ceil(nodes.length / perSector) * perSector * 128);
  for (let index = 0; index < out.length / 128; index += 1) {
    const at = index * 128;
    const each = nodes[index];
    out.writeUInt32LE(NONE, at + 68);
    out.writeUInt32LE(each?.right ?? NONE, at + 72);
    out.writeUInt32LE(each?.child ?? NONE, at + 76);
    if (!each) continue;
    const name = Buffer.from(`${each.name}\0`, 'utf16le');
    name.copy(out, at);
    out.writeUInt16LE(name.length, at + 64);
    out[at + 66] = each.type;
    out[at + 67] = 1;
    out.writeUInt32LE(each.type === 1 ? 0 : each.start, at + 116);
    out.writeUInt32LE(each.size, at + 120);
  }
  return out;
}

export interface CompoundOptions {
  /** Makes the chain of the stream at this path come back to its first sector. */
  cycle?: string;
  /** 3: sectors of 512 bytes, as HWP files have; 4: of 4096. */
  version?: 3 | 4;
}

/** How many FAT and DIFAT sectors a file of `used` other sectors takes. */
function tableCounts(used: number, size: number): { fats: number; difats: number } {
  const perSector = size / 4;
  let [fats, difats] = [0, 0];
  for (;;) {
    const neededFats = Math.ceil((used + fats + difats) / perSector);
    const neededDifats = Math.ceil(Math.max(0, neededFats - HEADER_FAT_SECTORS) / (perSector - 1));
    if (neededFats === fats && neededDifats === difats) return { fats, difats };
    [fats, difats] = [neededFats, neededDifats];
  }
}

/** The FAT's sectors and the DIFAT's, which list the FAT sectors past the header's 109. */
function tableSectors(
  fat: number[],
  size: number,
  first: number,
  counts: { fats: number; difats: number },
): Buffer[] {
  const perSector = size / 4;
  const difatStart = first + counts.fats;
  for (let index = 0; index < counts.fats; index += 1) fat.push(FAT_SECTOR);
  for (let index = 0; index < counts.difats; index += 1) fat.push(DIFAT_SECTOR);
  const fatBytes = Buffer.alloc(counts.fats * size, 0xff);
  fat.forEach((next, index) => fatBytes.writeUInt32LE(next, index * 4));
  const out: Buffer[] = [];
  for (let index = 0; index < counts.fats; index += 1) {
    out.push(fatBytes.subarray(index * size, (index + 1) * size));
  }
  for (let index = 0; index < counts.difats; index += 1) {
    const sector = Buffer.alloc(size, 0xff);
    for (let slot = 0; slot < perSector - 1; slot += 1) {
      const fatIndex = HEADER_FAT_SECTORS + index * (perSector - 1) + slot;
      if (fatIndex < counts.fats) sector.writeUInt32LE(first + fatIndex, slot * 4);
    }
    const last = index === counts.difats - 1;
    sector.writeUInt32LE(last ? END : difatStart + index + 1, (perSector - 1) * 4);
    out.push(sector);
  }
  return out;
}

/**
 * A compound file of the streams: those under 4096 bytes in the mini stream,
 * the others in sectors of their own, with DIFAT sectors when the FAT takes
 * more than the header lists.
 */
export function compoundFile(
  streams: readonly CompoundStream[],
  { cycle, version = 3 }: CompoundOptions = {},
): Buffer {
  const size = version === 3 ? 512 : 4096;
  const nodes = tree(streams);
  const leaves = nodes.filter((each) => each.stream);
  const sectors: Buffer[] = [];
  const fat: number[] = [];
  const miniParts: Buffer[] = [];
  const miniFat: number[] = [];
  for (const leaf of leaves) {
    const { stream } = leaf;
    if (!stream) continue;
    leaf.size = stream.said ?? stream.data.length;
    if (leaf.size < MINI_CUTOFF) {
      leaf.start = place(miniParts, miniFat, stream.data, MINI_SECTOR);
    } else {
      leaf.start = place(sectors, fat, stream.data, size);
      if (cycle === stream.path) fat[fat.length - 1] = leaf.start;
    }
  }
  for (const leaf of leaves) {
    const shared = leaves.find((each) => each.stream?.path === leaf.stream?.sharesWith);
    if (shared) leaf.start = shared.start;
  }
  const [root] = nodes;
  if (!root) throw new Error('no root entry');
  const miniStream = Buffer.concat(miniParts);
  root.start = place(sectors, fat, miniStream, size);
  root.size = miniStream.length;
  const miniFatBytes = Buffer.alloc(miniFat.length * 4);
  miniFat.forEach((next, index) => miniFatBytes.writeUInt32LE(next, index * 4));
  const miniFatStart = place(sectors, fat, miniFatBytes, size);
  const directoryBytes = directory(nodes, size);
  const directoryStart = place(sectors, fat, directoryBytes, size);
  const counts = tableCounts(sectors.length, size);
  const fatStart = sectors.length;
  sectors.push(...tableSectors(fat, size, fatStart, counts));
  const header = Buffer.alloc(size);
  Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]).copy(header);
  header.writeUInt16LE(0x3e, 24);
  header.writeUInt16LE(version, 26);
  header.writeUInt16LE(0xfffe, 28);
  header.writeUInt16LE(version === 3 ? 9 : 12, 30);
  header.writeUInt16LE(6, 32);
  header.writeUInt32LE(version === 3 ? 0 : directoryBytes.length / size, 40);
  header.writeUInt32LE(counts.fats, 44);
  header.writeUInt32LE(directoryStart, 48);
  header.writeUInt32LE(MINI_CUTOFF, 56);
  header.writeUInt32LE(miniFatStart, 60);
  header.writeUInt32LE(Math.ceil(miniFatBytes.length / size), 64);
  header.writeUInt32LE(counts.difats > 0 ? fatStart + counts.fats : END, 68);
  header.writeUInt32LE(counts.difats, 72);
  for (let index = 0; index < HEADER_FAT_SECTORS; index += 1) {
    header.writeUInt32LE(index < counts.fats ? fatStart + index : FREE, 76 + index * 4);
  }
  return Buffer.concat([header, ...sectors]);
}

/** Every stream of a compound file with its path, read with the viewer's own reader. */
export function compoundStreams(data: Buffer): CompoundStream[] {
  const file = readCompound(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
  const root = file?.entries[0];
  if (!file || !root) throw new Error('not a compound file');
  const walk = (storage: CompoundEntry, prefix: string): CompoundStream[] =>
    childrenOf(file, storage).flatMap((entry) => {
      const path = `${prefix}${entry.name}`;
      if (entry.type === COMPOUND_TYPES.STORAGE) return walk(entry, `${path}/`);
      const read = readStream(file, entry, entry.size);
      if (!read) throw new Error(`the stream ${path} does not read`);
      return [{ path, data: Buffer.from(read) }];
    });
  return walk(root, '');
}

/** A `FileHeader` stream of an HWP document: its signature, version 5.1, and `properties`. */
export function hwpFileHeader(properties = 0): Buffer {
  const header = Buffer.alloc(256);
  header.write('HWP Document File', 'ascii');
  header.writeUInt32LE(0x05010000, 32);
  header.writeUInt32LE(properties, 36);
  return header;
}
