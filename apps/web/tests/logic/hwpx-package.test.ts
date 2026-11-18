import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { MARKUP_LIMIT_BYTES } from '$lib/file-viewer/formats.ts';
import { HwpReader } from '../../src/viewer-frame/hwp-document.ts';
import {
  checkHwpxPackage,
  listed,
  masterPagesOf,
  sectionsOf,
} from '../../src/viewer-frame/hwpx-package.ts';
import { UnreadableFile } from '../../src/viewer-frame/renderer.ts';

import { koreanHwpx, rewriteArchive } from '../support/hwp-files.ts';
import { zip, type ZipMember } from '../support/viewer-files.ts';

const buffer = (data: Buffer): ArrayBuffer => Uint8Array.from(data).buffer;

/** A package list of the items (id, address) and a spine of the ids, as Hancom writes one. */
function contentHpf(items: [string, string][], spine: string[]): string {
  const manifest = items
    .map(([id, href]) => `<opf:item id="${id}" href="${href}" media-type="application/xml"/>`)
    .join('');
  const refs = spine.map((id) => `<opf:itemref idref="${id}" linear="yes"/>`).join('');
  return `<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>${manifest}</opf:manifest><opf:spine>${refs}</opf:spine></opf:package>`;
}

/** An HWPX archive: its mimetype, a package list, and the other parts. */
function hwpx(content: string, parts: ZipMember[]): Buffer {
  return zip([
    { name: 'mimetype', data: Buffer.from('application/hwp+zip'), stored: true },
    { name: 'Contents/content.hpf', data: Buffer.from(content) },
    ...parts,
  ]);
}

const xml = (bytes: number): Buffer =>
  Buffer.concat([Buffer.from('<hs:sec>'), Buffer.alloc(bytes, 0x20), Buffer.from('</hs:sec>')]);

describe("an HWPX package's list, read as rhwp reads it", () => {
  it('finds the items and the spine, whatever the prefix', () => {
    const list = listed(
      contentHpf(
        [
          ['header', 'Contents/header.xml'],
          ['s0', 'Contents/section0.xml'],
          ['mp', 'Contents/masterpage0.xml'],
        ],
        ['s0'],
      ),
    );
    expect(list.items.map((item) => item.href)).toEqual([
      'Contents/header.xml',
      'Contents/section0.xml',
      'Contents/masterpage0.xml',
    ]);
    expect(sectionsOf(list)).toEqual(['Contents/section0.xml']);
    expect(masterPagesOf(list)).toEqual(['Contents/masterpage0.xml']);
  });

  it('reads entities and quoted ">", and leaves comments out', () => {
    const list = listed(
      `<package><manifest><item id="s&#48;" href='Contents/section0.xml' title="a > b" media-type="application/xml"/>` +
        `<!-- <itemref idref="s0"/> --></manifest><spine><itemref note=">" idref="s&#x30;"/></spine></package>`,
    );
    expect(list.items).toEqual([
      { id: 's0', href: 'Contents/section0.xml', mediaType: 'application/xml' },
    ]);
    expect(list.spine).toEqual(['s0']);
  });

  it('without a spine, takes every XML part whose address says "section", in order', () => {
    const list = listed(
      contentHpf(
        [
          ['b', 'Contents/section1.xml'],
          ['a', 'Contents/section0.xml'],
        ],
        [],
      ),
    );
    expect(sectionsOf(list)).toEqual(['Contents/section0.xml', 'Contents/section1.xml']);
  });
});

describe('in the worker, what rhwp parses of an HWPX as XML is held to the limit', () => {
  it('the sample passes', async () => {
    await expect(checkHwpxPackage(buffer(koreanHwpx()))).resolves.toBeUndefined();
  });

  it('a spine that names a section twice stops the file', async () => {
    const twice = hwpx(contentHpf([['s0', 'Contents/section0.xml']], ['s0', 's0']), [
      { name: 'Contents/section0.xml', data: xml(10) },
    ]);
    await expect(checkHwpxPackage(buffer(twice))).rejects.toBeInstanceOf(UnreadableFile);
  });

  it('a section that does not start with "<" counts as XML all the same', async () => {
    const hidden = Buffer.concat([Buffer.from('x'), xml(MARKUP_LIMIT_BYTES)]);
    const archive = hwpx(contentHpf([['s0', 'Contents/section0.dat']], ['s0']), [
      { name: 'Contents/section0.dat', data: hidden },
    ]);
    await expect(checkHwpxPackage(buffer(archive))).rejects.toBeInstanceOf(UnreadableFile);
    // So does a part rhwp would not parse; pictures under BinData/ do not.
    const picture = hwpx(contentHpf([['s0', 'Contents/section0.xml']], ['s0']), [
      { name: 'Contents/section0.xml', data: xml(10) },
      { name: 'BinData/image1.png', data: Buffer.alloc(MARKUP_LIMIT_BYTES + 1) },
    ]);
    await expect(checkHwpxPackage(buffer(picture))).resolves.toBeUndefined();
  });

  it('master pages count once for each section', async () => {
    const sections = (count: number): [string, string][] =>
      Array.from({ length: count }, (_, index) => [`s${index}`, `Contents/section${index}.xml`]);
    const archive = (count: number): Buffer =>
      hwpx(
        contentHpf(
          [...sections(count), ['mp', 'Contents/masterpage0.xml']],
          sections(count).map(([id]) => id),
        ),
        [
          ...sections(count).map(([, name]) => ({ name, data: xml(10) })),
          { name: 'Contents/masterpage0.xml', data: xml(1024 * 1024) },
        ],
      );
    await expect(checkHwpxPackage(buffer(archive(20)))).resolves.toBeUndefined();
    await expect(checkHwpxPackage(buffer(archive(40)))).rejects.toBeInstanceOf(UnreadableFile);
  });
});

/** rhwp's WebAssembly, as the frame hands it to the worker. */
const WASM = readFileSync(
  path.join(path.dirname(createRequire(import.meta.url).resolve('@rhwp/core')), 'rhwp_bg.wasm'),
).toString('hex');

describe("the worker's reader holds an HWPX to the limits before rhwp reads it", () => {
  const open = (data: Buffer) =>
    new HwpReader().answer({ kind: 'open', format: 'hwpx', bytes: buffer(data), wasm: WASM });
  /** The sample with its section rewritten by `change`. */
  const sample = (change: (member: ZipMember) => ZipMember): Buffer =>
    rewriteArchive(koreanHwpx(), (member) =>
      member.name === 'Contents/section0.xml' || member.name === 'Contents/content.hpf'
        ? change(member)
        : member,
    );

  it('opens the sample as it is, and as rewritten unchanged', async () => {
    expect((await open(sample((member) => member))).kind).toBe('opened');
  });

  it('stops the sample when its section unpacks to more than it says', async () => {
    const lying = sample((member) =>
      member.name === 'Contents/section0.xml' ? { ...member, said: 100 } : member,
    );
    expect(await open(lying)).toEqual({ kind: 'failed', reason: 'unreadable' });
  });

  it('stops the sample when its spine names its section twice', async () => {
    const twice = sample((member) => {
      if (member.name !== 'Contents/content.hpf') return member;
      const text = member.data.toString('utf8');
      const ref = /<opf:itemref idref="section0"[^>]*\/>/.exec(text)?.[0] ?? '';
      expect(ref).not.toBe('');
      return { ...member, data: Buffer.from(text.replace(ref, ref + ref)) };
    });
    expect(await open(twice)).toEqual({ kind: 'failed', reason: 'unreadable' });
  });
});
