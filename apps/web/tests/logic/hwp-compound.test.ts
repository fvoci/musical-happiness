import { constants, deflateRawSync, deflateSync } from 'node:zlib';

import { describe, expect, it } from 'vitest';

import { compoundFile, verifyChains } from '$lib/file-viewer/compound-file.ts';
import { examine, UNPACKED_LIMIT_BYTES, type FormatId } from '$lib/file-viewer/formats.ts';
import { checkCompound } from '../../src/viewer-frame/compound-guard.ts';
import { LockedFile, TooLargeFile, UnreadableFile } from '../../src/viewer-frame/renderer.ts';

import {
  compoundFile as writeCompound,
  compoundStreams,
  hwpFileHeader,
  koreanHwp,
  lockedHwp,
  type CompoundStream,
} from '../support/hwp-files.ts';
import { zip } from '../support/viewer-files.ts';

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

describe('the app answers "too large" from what the streams say, before anything unpacks', () => {
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
  it('an honest one is handed on as it came', async () => {
    const honest = buffer(koreanHwp());
    await expect(checkCompound(honest)).resolves.toBe(honest);
    await expect(checkCompound(buffer(hwpFile()))).resolves.toBeInstanceOf(ArrayBuffer);
  });

  it('a stream that says more than its chain holds stops the file', async () => {
    await expect(checkCompound(buffer(hwpFile({ said: 20_000 })))).rejects.toThrow(UnreadableFile);
  });

  it('two streams on one chain, or a chain that comes back on itself, stop the file', async () => {
    const shared = writeCompound([
      { path: 'FileHeader', data: hwpFileHeader() },
      { path: 'BodyText/Section0', data: Buffer.alloc(9000, 2) },
      { path: 'BodyText/Section1', data: Buffer.alloc(9000, 3), sharesWith: 'BodyText/Section0' },
    ]);
    const file = compoundFile(bytes(shared));
    expect(file && verifyChains(file)).toBe(false);
    await expect(checkCompound(buffer(shared))).rejects.toThrow(UnreadableFile);
    const cycle = hwpFile({}, 'BodyText/Section0');
    await expect(checkCompound(buffer(cycle))).rejects.toThrow(UnreadableFile);
  });

  it('a document locked with a password says so', async () => {
    const locked = writeCompound([{ path: 'FileHeader', data: hwpFileHeader(0b11) }]);
    await expect(checkCompound(buffer(locked))).rejects.toThrow(LockedFile);
    await expect(checkCompound(buffer(lockedHwp()))).rejects.toThrow(LockedFile);
  });
});

describe('in the frame, what the streams unpack to is held to the limit', () => {
  const LIMIT = 1 << 20;
  /** An HWP document with a picture stream of these bytes. */
  const withPicture = (...pictures: Buffer[]): ArrayBuffer =>
    buffer(
      writeCompound([
        { path: 'FileHeader', data: hwpFileHeader(1) },
        { path: 'DocInfo', data: deflateRawSync(Buffer.alloc(8000, 1)) },
        ...pictures.map((data, index) => ({ path: `BinData/BIN000${String(index)}.bmp`, data })),
      ]),
    );
  const zeros = Buffer.alloc(4 << 20);

  it('a picture deflated past it, raw or in zlib, stops the file', async () => {
    await expect(checkCompound(withPicture(deflateRawSync(zeros)), LIMIT)).rejects.toThrow(
      TooLargeFile,
    );
    await expect(checkCompound(withPicture(deflateSync(zeros)), LIMIT)).rejects.toThrow(
      TooLargeFile,
    );
  });

  it('what comes out before a stream fails counts: with a byte after it, or a broken block', async () => {
    const part = Buffer.alloc(600 << 10);
    const junk = Buffer.concat([deflateRawSync(part), Buffer.from([0x42])]);
    // Blocks that do not end the stream, then one of a kind that does not exist.
    const open = deflateRawSync(part, { finishFlush: constants.Z_SYNC_FLUSH });
    const broken = Buffer.concat([open, Buffer.from([0x07, 0, 0, 0])]);
    await expect(checkCompound(withPicture(junk), LIMIT)).resolves.toBeInstanceOf(ArrayBuffer);
    await expect(checkCompound(withPicture(junk, broken), LIMIT)).rejects.toThrow(TooLargeFile);
  });

  it('a stream that is not deflated counts as it is, and the streams count together', async () => {
    const stored = Buffer.alloc(600 << 10, 0x89);
    await expect(checkCompound(withPicture(stored), LIMIT)).resolves.toBeInstanceOf(ArrayBuffer);
    await expect(checkCompound(withPicture(stored, stored), LIMIT)).rejects.toThrow(TooLargeFile);
    await expect(
      checkCompound(withPicture(deflateRawSync(Buffer.alloc(900 << 10))), LIMIT),
    ).resolves.toBeInstanceOf(ArrayBuffer);
  });
});

describe('a broken compound file is nothing, not an error', () => {
  /** Where entry `index` of the directory starts, in a file written by the test writer. */
  const entryAt = (file: Buffer, index: number): number => {
    const size = 1 << file.readUInt16LE(30);
    return (file.readUInt32LE(48) + 1) * size + index * 128;
  };

  it('a FAT sector past the end of the file, or a DIFAT chain that ends too soon', () => {
    const far = hwpFile();
    far.writeUInt32LE(0x7fffffff, 76);
    expect(compoundFile(bytes(far))).toBeUndefined();
    const picture = { path: 'BinData/BIN0009.bmp', data: Buffer.alloc(8 << 20, 9) };
    const large = writeCompound([...compoundStreams(koreanHwp()), picture]);
    expect(compoundFile(bytes(large))).toBeDefined();
    large.writeUInt32LE(0xfffffffe, 68);
    expect(compoundFile(bytes(large))).toBeUndefined();
  });

  it('a version 4 file with the high half of a size set', () => {
    const file = writeCompound([{ path: 'FileHeader', data: hwpFileHeader() }], { version: 4 });
    expect(compoundFile(bytes(file))).toBeDefined();
    file.writeUInt32LE(1, entryAt(file, 1) + 124);
    expect(compoundFile(bytes(file))).toBeUndefined();
  });

  it("the root's mini stream: shared with a stream, larger than its chain, or one chain for two", () => {
    const honest = hwpFile();
    const root = entryAt(honest, 0);
    const section = compoundFile(bytes(honest))?.entries.find((entry) => entry.name === 'Section0');
    if (!section) throw new Error('no section');
    const verified = (file: Buffer): boolean | undefined => {
      const read = compoundFile(bytes(file));
      return read && verifyChains(read);
    };
    expect(verified(honest)).toBe(true);
    const sharing = Buffer.from(honest);
    sharing.writeUInt32LE(section.start, root + 116);
    expect(verified(sharing)).toBe(false);
    const larger = Buffer.from(honest);
    larger.writeUInt32LE(1 << 20, root + 120);
    expect(verified(larger)).toBe(false);
    const twoMini = writeCompound([
      { path: 'FileHeader', data: hwpFileHeader() },
      { path: 'DocInfo', data: Buffer.alloc(1000, 1) },
      { path: 'PrvText', data: Buffer.alloc(1000, 2), sharesWith: 'DocInfo' },
    ]);
    expect(verified(twoMini)).toBe(false);
  });
});

describe('an archive or a compound file is told by what it holds, not as a PDF', () => {
  const pdf = Buffer.from('%PDF-1.7\n%%EOF\n');

  it('a compound file with a PDF picture stored early', () => {
    const file = writeCompound([
      { path: 'FileHeader', data: hwpFileHeader() },
      { path: 'BinData/BIN0001.pdf', data: Buffer.concat([pdf, Buffer.alloc(5000)]) },
    ]);
    expect(file.subarray(0, 1024).includes(pdf.subarray(0, 5))).toBe(true);
    expect(sniffFormat(file)).toBe('hwp');
  });

  it('an archive with a PDF part stored early', () => {
    const archive = zip([
      { name: 'mimetype', data: Buffer.from('application/hwp+zip'), stored: true },
      { name: 'BinData/a.pdf', data: pdf, stored: true },
      { name: 'Contents/section0.xml', data: Buffer.from('<hs:sec/>') },
    ]);
    expect(archive.subarray(0, 1024).includes(pdf.subarray(0, 5))).toBe(true);
    expect(sniffFormat(archive)).toBe('hwpx');
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

  it('version 4 (sectors of 4096 bytes)', async () => {
    const again = writeCompound(sample, { version: 4 });
    expect(again.readUInt16LE(26)).toBe(4);
    expect(sniffFormat(again)).toBe('hwp');
    await expect(checkCompound(buffer(again))).resolves.toBeInstanceOf(ArrayBuffer);
    expect(listed(compoundStreams(again))).toEqual(listed(sample));
  });

  it('a FAT longer than the header lists, continued in DIFAT sectors', async () => {
    const picture = { path: 'BinData/BIN0009.bmp', data: Buffer.alloc(8 << 20, 9) };
    const large = writeCompound([...sample, picture]);
    expect(large.readUInt32LE(72)).toBeGreaterThan(0);
    expect(sniffFormat(large)).toBe('hwp');
    await expect(checkCompound(buffer(large))).resolves.toBeInstanceOf(ArrayBuffer);
    expect(listed(compoundStreams(large))).toEqual(listed([...sample, picture]));
  });
});
