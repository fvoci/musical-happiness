import { constants, deflateRawSync, deflateSync, inflateRawSync, inflateSync } from 'node:zlib';

import { describe, expect, it } from 'vitest';

import { deflatedSize } from '../../src/viewer-frame/deflated-size.ts';

const SYNC = { finishFlush: constants.Z_SYNC_FLUSH };

/** Numbers from a seed, the same each run. */
function numbers(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let mixed = Math.imul(state ^ (state >>> 15), 1 | state);
    mixed = (mixed + Math.imul(mixed ^ (mixed >>> 7), 61 | mixed)) ^ mixed;
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 2 ** 32;
  };
}

function noise(length: number, next: () => number): Buffer {
  return Buffer.from(Array.from({ length }, () => Math.floor(next() * 256)));
}

const korean = Buffer.from('회의록: 설계 검토, 김민지. 다음 주까지 항목을 정리한다. '.repeat(2000));
const SAMPLES: [string, Buffer][] = [
  ['Korean text', korean],
  ['noise', noise(30_000, numbers(1))],
  ['zeros', Buffer.alloc(300_000)],
  ['text and noise', Buffer.concat([korean.subarray(0, 40_000), noise(9000, numbers(2)), korean])],
];
const STRATEGIES = [
  constants.Z_DEFAULT_STRATEGY,
  constants.Z_FILTERED,
  constants.Z_HUFFMAN_ONLY,
  constants.Z_RLE,
  constants.Z_FIXED,
];

/** What zlib unpacks of `data` before it fails: as far as the longest start of it that it reads. */
function zlibUnpacks(data: Buffer, zlib = false): number {
  const unpack = (part: Buffer): number =>
    (zlib ? inflateSync(part, SYNC) : inflateRawSync(part, SYNC)).length;
  let [low, high] = [0, data.length];
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    try {
      unpack(data.subarray(0, middle));
      low = middle;
    } catch {
      high = middle - 1;
    }
  }
  try {
    return unpack(data.subarray(0, low));
  } catch {
    return 0;
  }
}

describe('what a deflated stream unpacks to, counted without unpacking it', () => {
  it('is what zlib unpacks, at every level and way of packing', () => {
    for (const [, plain] of SAMPLES) {
      for (const level of [0, 1, 6, 9]) {
        for (const strategy of STRATEGIES) {
          const packed = deflateRawSync(plain, { level, strategy });
          expect(deflatedSize(packed, 0, Infinity)).toBe(plain.length);
        }
      }
    }
  });

  it('passes over the bytes after the last block, as rhwp does; a browser fails on them', () => {
    for (const [, plain] of SAMPLES) {
      const after = Buffer.concat([deflateRawSync(plain), Buffer.from([0x07, 0x00, 0xff])]);
      expect(deflatedSize(after, 0, Infinity)).toBe(plain.length);
      expect(inflateRawSync(after).length).toBe(plain.length);
    }
  });

  it('counts a stream without its last block, or cut short, as far as zlib reads it', () => {
    const next = numbers(3);
    for (const [, plain] of SAMPLES) {
      const open = deflateRawSync(plain, SYNC);
      expect(deflatedSize(open, 0, Infinity)).toBe(plain.length);
      for (let cut = 0; cut < 12; cut += 1) {
        const short = open.subarray(0, Math.floor(next() * open.length));
        expect(deflatedSize(short, 0, Infinity)).toBe(inflateRawSync(short, SYNC).length);
      }
    }
  });

  it('reads a zlib stream after its two bytes of header', () => {
    for (const [, plain] of SAMPLES) {
      const packed = deflateSync(plain, { level: 9 });
      expect(deflatedSize(packed, 2, Infinity)).toBe(inflateSync(packed).length);
    }
  });

  it('is never less than zlib unpacks of a broken stream, or of bytes that are none', () => {
    const next = numbers(4);
    for (let round = 0; round < 300; round += 1) {
      const [, plain] = SAMPLES[round % SAMPLES.length] ?? ['', korean];
      const packed = Buffer.from(deflateRawSync(plain.subarray(0, 20_000), { level: round % 10 }));
      for (let flip = 0; flip < 1 + (round % 4); flip += 1) {
        const at = Math.floor(next() * packed.length);
        packed[at] = (packed[at] ?? 0) ^ (1 << Math.floor(next() * 8));
      }
      expect(deflatedSize(packed, 0, Infinity)).toBeGreaterThanOrEqual(zlibUnpacks(packed));
      const bytes = noise(1 + Math.floor(next() * 2000), next);
      expect(deflatedSize(bytes, 0, Infinity)).toBeGreaterThanOrEqual(zlibUnpacks(bytes));
      expect(deflatedSize(bytes, 2, Infinity)).toBeGreaterThanOrEqual(
        zlibUnpacks(Buffer.concat([Buffer.from([0x78, 0x9c]), bytes.subarray(2)]), true),
      );
    }
  });

  it('reads on where zlib refuses: a copy from before the start, a stored length its complement denies', () => {
    // A block of fixed codes that starts with a copy of 258 bytes from one byte back, 1,000 times.
    const bits: number[] = [1, 1, 0];
    const code = (value: number, length: number): void => {
      for (let bit = length - 1; bit >= 0; bit -= 1) bits.push((value >> bit) & 1);
    };
    for (let copy = 0; copy < 1000; copy += 1) {
      code(0b11000101, 8);
      code(0, 5);
    }
    code(0, 7);
    const far = Buffer.alloc(Math.ceil(bits.length / 8));
    bits.forEach((bit, at) => {
      far[at >> 3] = (far[at >> 3] ?? 0) | (bit << (at & 7));
    });
    expect(() => inflateRawSync(far)).toThrow(/too far back/);
    expect(deflatedSize(far, 0, Infinity)).toBe(258_000);
    const stored = Buffer.from([0x01, 0x05, 0x00, 0x00, 0x00, 1, 2, 3, 4, 5]);
    expect(() => inflateRawSync(stored)).toThrow(/stored block lengths/);
    expect(deflatedSize(stored, 0, Infinity)).toBe(5);
  });

  it('stops at a block of the reserved kind, and just past the room it is given', () => {
    const open = deflateRawSync(Buffer.alloc(5000, 1), SYNC);
    expect(deflatedSize(Buffer.concat([open, Buffer.from([0x07])]), 0, Infinity)).toBe(5000);
    const bomb = deflateRawSync(Buffer.alloc(64 << 20));
    const counted = deflatedSize(bomb, 0, 1 << 20);
    expect(counted).toBeGreaterThan(1 << 20);
    expect(counted).toBeLessThan((1 << 20) + 300);
  });

  it('reads a stream of many blocks of their own codes that unpack to nothing in a moment', () => {
    // Each block gives 258 lengths (two literals of 1 bit, one distance) in 91 bits, then ends.
    // 1.6 million of them (18 MB, under the file limit) took a counter that made its tables
    // anew for each block 33 s, past the worker's time.
    const bits: number[] = [];
    const put = (value: number, count: number): void => {
      for (let bit = 0; bit < count; bit += 1) bits.push((value >> bit) & 1);
    };
    const order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1];
    for (let block = 0; block < 8; block += 1) {
      put(0b100, 3);
      put(0, 10);
      put(14, 4);
      for (const symbol of order) put(symbol === 18 || symbol === 1 ? 1 : 0, 3);
      // Lengths 1, 0 × 138, 0 × 117, 1 and 1 (symbol 1 is "0", 18 is "1"); then the end ("1").
      for (const [code, extra] of [[0], [1, 127], [1, 106], [0], [0], [1]] as const) {
        put(code, 1);
        if (extra !== undefined) put(extra, 7);
      }
    }
    const eight = Buffer.alloc(Math.ceil(bits.length / 8));
    bits.forEach((bit, at) => {
      eight[at >> 3] = (eight[at >> 3] ?? 0) | (bit << (at & 7));
    });
    const many = Buffer.concat(Array.from({ length: 200_000 }, () => eight));
    expect(inflateRawSync(many, SYNC).length).toBe(0);
    const started = performance.now();
    expect(deflatedSize(many, 0, Infinity)).toBe(0);
    expect(performance.now() - started).toBeLessThan(10_000);
  }, 60_000);
});
