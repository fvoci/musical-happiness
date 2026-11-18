import { describe, expect, it } from 'vitest';

import { compoundFile, verifyChains } from '$lib/file-viewer/compound-file.ts';
import { examine, UNPACKED_LIMIT_BYTES, type FormatId } from '$lib/file-viewer/formats.ts';
import { checkCompound } from '../../src/viewer-frame/compound-guard.ts';
import { LockedFile, UnreadableFile } from '../../src/viewer-frame/renderer.ts';

import {
  compoundFile as writeCompound,
  compoundStreams,
  hwpFileHeader,
  koreanHwp,
  lockedHwp,
  type CompoundStream,
} from '../support/hwp-files.ts';

const bytes = (buffer: Buffer): Uint8Array =>
  new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
const buffer = (data: Buffer): ArrayBuffer => Uint8Array.from(data).buffer;
const sniffFormat = (data: Buffer): FormatId | undefined => {
  const examined = examine(bytes(data));
  return examined.kind === 'format' ? examined.format : undefined;
};

/** An HWP document of a header and a section, with what the streams say about themselves. */
const hwpFile = (section: Partial<CompoundStream> = {}, cycle?: string): Buffer =>
  writeCompound(
    [
      { path: 'FileHeader', data: hwpFileHeader(1) },
      { path: 'DocInfo', data: Buffer.alloc(8000, 1) },
      { path: 'BodyText/Section0', data: Buffer.alloc(9000, 2), ...section },
    ],
    { cycle },
  );

describe('HWP is told apart by its first bytes and its streams', () => {
  it('HWP: a compound file whose FileHeader starts with "HWP Document File"', () => {
    expect(sniffFormat(koreanHwp())).toBe('hwp');
    expect(sniffFormat(hwpFile())).toBe('hwp');
  });

  it('another compound file (an old Word or Excel file) is none of them', () => {
    const word = writeCompound([
      { path: 'WordDocument', data: Buffer.alloc(5000, 7) },
      { path: '\u0001CompObj', data: Buffer.alloc(100, 1) },
    ]);
    const excel = writeCompound([{ path: 'Workbook', data: Buffer.alloc(300, 3) }]);
    expect(compoundFile(bytes(word))).toBeDefined();
    expect(sniffFormat(word)).toBeUndefined();
    expect(sniffFormat(excel)).toBeUndefined();
  });

  it('a FileHeader with another signature, or inside a storage, is no HWP', () => {
    const other = Buffer.from(hwpFileHeader());
    other.write('HWP Document Fill', 'ascii');
    expect(sniffFormat(writeCompound([{ path: 'FileHeader', data: other }]))).toBeUndefined();
    const nested = writeCompound([{ path: 'Hidden/FileHeader', data: hwpFileHeader() }]);
    expect(sniffFormat(nested)).toBeUndefined();
  });

  it('the signature of a compound file alone, or a broken one, is nothing', () => {
    const signature = Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    expect(sniffFormat(Buffer.concat([signature, Buffer.alloc(2000)]))).toBeUndefined();
    const broken = hwpFile();
    broken.writeUInt32LE(0xfffffff0, 48); // the directory's first sector
    expect(sniffFormat(broken)).toBeUndefined();
  });
});

describe('an HWP document is held to what its streams say they hold', () => {
  it('together up to 200 MB; past that it is offered as a download', () => {
    // The header and DocInfo say their own 256 + 8000 bytes; the section says the rest.
    const at = hwpFile({ said: UNPACKED_LIMIT_BYTES - 8256 });
    const past = hwpFile({ said: UNPACKED_LIMIT_BYTES - 8255 });
    expect(examine(bytes(at))).toEqual({ kind: 'format', format: 'hwp' });
    expect(examine(bytes(past))).toEqual({ kind: 'too-large', format: 'hwp' });
  });

  it('many streams that share one chain count each time', () => {
    const streams: CompoundStream[] = [
      { path: 'FileHeader', data: hwpFileHeader() },
      { path: 'BinData/BIN0000.png', data: Buffer.alloc(1 << 20, 5) },
    ];
    for (let index = 1; index < 200; index += 1) {
      const name = `BinData/BIN${String(index).padStart(4, '0')}.png`;
      streams.push({
        path: name,
        data: Buffer.alloc(4096),
        sharesWith: 'BinData/BIN0000.png',
        said: 1 << 20,
      });
    }
    const shared = writeCompound(streams);
    expect(shared.length).toBeLessThan(4 << 20);
    expect(examine(bytes(shared))).toEqual({ kind: 'too-large', format: 'hwp' });
  });
});

describe('in the frame, a compound file is checked before rhwp reads it', () => {
  it('an honest one is handed on as it came', () => {
    const honest = buffer(koreanHwp());
    expect(checkCompound(honest)).toBe(honest);
    expect(checkCompound(buffer(hwpFile()))).toBeInstanceOf(ArrayBuffer);
  });

  it('a stream that says more than its chain holds stops the file', () => {
    expect(() => checkCompound(buffer(hwpFile({ said: 20_000 })))).toThrow(UnreadableFile);
  });

  it('two streams on one chain, or a chain that comes back on itself, stop the file', () => {
    const shared = writeCompound([
      { path: 'FileHeader', data: hwpFileHeader() },
      { path: 'BodyText/Section0', data: Buffer.alloc(9000, 2) },
      { path: 'BodyText/Section1', data: Buffer.alloc(9000, 3), sharesWith: 'BodyText/Section0' },
    ]);
    const file = compoundFile(bytes(shared));
    expect(file && verifyChains(file)).toBe(false);
    expect(() => checkCompound(buffer(shared))).toThrow(UnreadableFile);
    expect(() => checkCompound(buffer(hwpFile({}, 'BodyText/Section0')))).toThrow(UnreadableFile);
  });

  it('a document locked with a password says so', () => {
    const locked = writeCompound([{ path: 'FileHeader', data: hwpFileHeader(0b11) }]);
    expect(() => checkCompound(buffer(locked))).toThrow(LockedFile);
    expect(() => checkCompound(buffer(lockedHwp()))).toThrow(LockedFile);
  });
});

describe('the reader takes a compound file of either version, and a FAT past the header', () => {
  const listed = (streams: readonly CompoundStream[]): string[][] =>
    streams.map((stream) => [stream.path, stream.data.toString('hex')]).sort();
  const sample = compoundStreams(koreanHwp());

  it('reads every stream of the sample', () => {
    const paths = sample.map((stream) => stream.path);
    for (const path of ['FileHeader', 'DocInfo', 'BodyText/Section0'])
      expect(paths).toContain(path);
  });

  it('version 4 (sectors of 4096 bytes)', () => {
    const again = writeCompound(sample, { version: 4 });
    expect(again.readUInt16LE(26)).toBe(4);
    expect(sniffFormat(again)).toBe('hwp');
    expect(checkCompound(buffer(again))).toBeInstanceOf(ArrayBuffer);
    expect(listed(compoundStreams(again))).toEqual(listed(sample));
  });

  it('a FAT longer than the header lists, continued in DIFAT sectors', () => {
    const picture = { path: 'BinData/BIN0009.bmp', data: Buffer.alloc(8 << 20, 9) };
    const large = writeCompound([...sample, picture]);
    expect(large.readUInt32LE(72)).toBeGreaterThan(0);
    expect(sniffFormat(large)).toBe('hwp');
    expect(checkCompound(buffer(large))).toBeInstanceOf(ArrayBuffer);
    expect(listed(compoundStreams(large))).toEqual(listed([...sample, picture]));
  });
});
