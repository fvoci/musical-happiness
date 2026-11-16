import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { sniffFormat } from '$lib/file-viewer/formats.ts';
import {
  PAGE_LIMIT,
  readFromFrame,
  readToFrame,
  VIEWER_KEYS,
  ZOOM_MAX,
  ZOOM_MIN,
} from '$lib/file-viewer/protocol.ts';
import { zipMemberNames } from '$lib/file-viewer/zip-names.ts';

import { koreanDocx, koreanPdf, png, zip } from '../support/viewer-files.ts';

const bytes = (buffer: Buffer): Uint8Array =>
  new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
const text = (value: string): Uint8Array => new TextEncoder().encode(value);

describe('a file is told apart by its first bytes, not by its name', () => {
  it('a PDF', () => {
    expect(sniffFormat(bytes(koreanPdf(2)))).toBe('pdf');
  });

  it('a PDF with something before its header, within the first 1024 bytes', () => {
    expect(sniffFormat(text(`${' '.repeat(1000)}%PDF-1.4\n`))).toBe('pdf');
    expect(sniffFormat(text(`${' '.repeat(1100)}%PDF-1.4\n`))).toBeUndefined();
  });

  it('a Word document: a ZIP archive with word/document.xml', () => {
    expect(sniffFormat(bytes(koreanDocx()))).toBe('docx');
  });

  it('another ZIP archive, a picture, text and nothing are none of them', () => {
    const archive = zip([
      { name: 'xl/workbook.xml', data: Buffer.from('<workbook/>') },
      { name: 'word/document.xml.bak', data: Buffer.from('x') },
    ]);
    expect(sniffFormat(bytes(archive))).toBeUndefined();
    expect(sniffFormat(bytes(png(4, 4, [0, 0, 0])))).toBeUndefined();
    expect(sniffFormat(text('%PD'))).toBeUndefined();
    expect(sniffFormat(text('plain text, named report.pdf'))).toBeUndefined();
    expect(sniffFormat(new Uint8Array())).toBeUndefined();
  });
});

describe('the names of a ZIP archive', () => {
  it('are read from its directory, in order', () => {
    const archive = zip([
      { name: '[Content_Types].xml', data: Buffer.from('a') },
      { name: '한글/이름.xml', data: Buffer.from('b') },
    ]);
    expect(zipMemberNames(bytes(archive))).toEqual(['[Content_Types].xml', '한글/이름.xml']);
  });

  it('are found after a comment at the end of the archive', () => {
    const archive = zip([{ name: 'word/document.xml', data: Buffer.from('<w/>') }]);
    const commented = Buffer.concat([archive, Buffer.from('a comment')]);
    commented.writeUInt16LE(9, archive.length - 2);
    expect(zipMemberNames(bytes(commented))).toEqual(['word/document.xml']);
  });

  it('are not read from bytes that only look like an archive', () => {
    const archive = zip([{ name: 'word/document.xml', data: Buffer.from('<w/>') }]);
    // The directory said to lie past the end of the file.
    const broken = Buffer.from(archive);
    broken.writeUInt32LE(archive.length, archive.length - 6);
    expect(zipMemberNames(bytes(broken))).toBeUndefined();
    expect(zipMemberNames(bytes(archive.subarray(0, archive.length - 30)))).toBeUndefined();
  });

  it('reading never throws, whatever the bytes', () => {
    const archive = zip([{ name: 'word/document.xml', data: Buffer.from('<w/>') }]);
    fc.assert(
      fc.property(fc.uint8Array({ maxLength: 200 }), fc.nat(archive.length - 1), (noise, at) => {
        const mixed = Uint8Array.from(archive);
        mixed.set(noise.subarray(0, archive.length - at), at);
        expect(() => sniffFormat(mixed)).not.toThrow();
        expect(() => zipMemberNames(noise)).not.toThrow();
      }),
    );
  });
});

describe('a message from the frame is taken only in one of its shapes', () => {
  it('each kind', () => {
    expect(readFromFrame({ kind: 'ready' })).toEqual({ kind: 'ready' });
    expect(readFromFrame({ kind: 'shown', pages: 3 })).toEqual({ kind: 'shown', pages: 3 });
    expect(readFromFrame({ kind: 'page', page: 2 })).toEqual({ kind: 'page', page: 2 });
    expect(readFromFrame({ kind: 'failed', reason: 'locked' })).toEqual({
      kind: 'failed',
      reason: 'locked',
    });
    for (const key of VIEWER_KEYS) {
      expect(readFromFrame({ kind: 'key', key })).toEqual({ kind: 'key', key });
    }
  });

  it('not with a field more, a field less, or a value out of range', () => {
    for (const data of [
      undefined,
      null,
      'ready',
      ['ready'],
      {},
      { kind: 'ready', extra: 1 },
      { kind: 'shown' },
      { kind: 'shown', pages: 0 },
      { kind: 'shown', pages: 1.5 },
      { kind: 'shown', pages: PAGE_LIMIT + 1 },
      { kind: 'shown', pages: '3' },
      { kind: 'page', page: -1 },
      { kind: 'failed', reason: 'because' },
      { kind: 'key', key: 'Enter' },
      { kind: 'key', key: 'a' },
      { kind: 'open', format: 'pdf', bytes: new ArrayBuffer(1), theme: 'light' },
      { kind: 'navigate', url: 'https://example.com/' },
    ]) {
      expect(readFromFrame(data), JSON.stringify(data)).toBeUndefined();
    }
  });

  it('not from an object that is not plain', () => {
    class Shaped {
      kind = 'ready';
    }
    expect(readFromFrame(new Shaped())).toBeUndefined();
    expect(readFromFrame(Object.assign(Object.create(null) as object, { kind: 'ready' }))).toBe(
      undefined,
    );
  });
});

describe('a message from the app, in the frame', () => {
  it('opens a file of a kind the viewer draws, with its bytes', () => {
    const buffer = new ArrayBuffer(4);
    const open = { kind: 'open', format: 'docx', bytes: buffer, theme: 'dark', lang: 'ko' };
    expect(readToFrame(open)).toEqual(open);
    expect(readToFrame({ ...open, lang: 'zh-Hans' })).toEqual({ ...open, lang: 'zh-Hans' });
    for (const wrong of [
      { ...open, format: 'html' },
      { ...open, bytes: new Uint8Array(4) },
      { ...open, lang: 'ko"><script>' },
      { ...open, lang: '' },
      { kind: 'open', format: 'docx', bytes: buffer, theme: 'dark' },
    ]) {
      expect(readToFrame(wrong), JSON.stringify(wrong)).toBeUndefined();
    }
  });

  it('zooms within its range and goes to a page', () => {
    expect(readToFrame({ kind: 'zoom', zoom: ZOOM_MIN })).toEqual({ kind: 'zoom', zoom: ZOOM_MIN });
    expect(readToFrame({ kind: 'zoom', zoom: ZOOM_MAX * 2 })).toBeUndefined();
    expect(readToFrame({ kind: 'zoom', zoom: Number.NaN })).toBeUndefined();
    expect(readToFrame({ kind: 'page', page: 4 })).toEqual({ kind: 'page', page: 4 });
    expect(readToFrame({ kind: 'theme', theme: 'dim' })).toBeUndefined();
  });
});
