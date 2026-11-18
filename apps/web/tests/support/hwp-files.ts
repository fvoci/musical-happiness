/**
 * Files for the tests of the HWP viewer, made here rather than kept as
 * binaries: an HWPX document in Korean with a header, a table, a picture and
 * a second page, written by rhwp's own exporter (`@rhwp/core`, the viewer's
 * renderer, run in Node). Every file is ours, so the tests carry no
 * third-party content.
 */
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { inflateRawSync } from 'node:zlib';

import { HwpDocument, initSync } from '@rhwp/core';

import { zipEntries } from '../../src/lib/file-viewer/zip-directory.ts';

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

function startRhwp(): void {
  if (ready) return;
  const require = createRequire(import.meta.url);
  const folder = path.dirname(require.resolve('@rhwp/core'));
  initSync({ module: readFileSync(path.join(folder, 'rhwp_bg.wasm')) });
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

export function lockedHwpx(): Buffer {
  return Buffer.from(koreanDocument().exportHwpxWithPassword(HWP_PASSWORD));
}
