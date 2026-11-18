/**
 * How much a deflated stream unpacks to, counted by reading its codes
 * without unpacking it: what the compound guard (compound-guard.ts) holds
 * an HWP document's streams to before rhwp reads them.
 *
 * rhwp (0.8.7) unpacks a stream with flate2's readers, which decode as zlib
 * does (zlib-rs): as raw deflate, then as zlib. A reader stops at the end of
 * the last block and passes over whatever follows it, and takes a stream cut
 * short as far as it goes. Hancom's own streams have bytes after their last
 * block; the browser's `DecompressionStream` fails on those, and drops what
 * the slice it failed in unpacked to, so it cannot count what rhwp takes.
 *
 * What a stream unpacks to depends on its codes alone, not on the bytes they
 * stand for, so nothing is written here: a literal counts one byte and a copy
 * its length. The count is never less than what zlib unpacks. What zlib
 * refuses past the point it stops at (a copy from before the start, a table
 * of codes left incomplete, lengths that say more than there is room for) is
 * read on here; only what no reader can read past ends the count early: a
 * block of the reserved type, a code that stands for nothing, a table with
 * more codes than its lengths have room for, a symbol past the alphabet.
 */

/** Thrown when the stream ends, or goes on in a way no reader can follow; one, made once. */
class StreamEnd extends Error {}
const END = new StreamEnd('the deflated stream ends');

/** The longest code, in bits. */
const MAX_BITS = 15;
/** Codes up to this long are looked up at once; longer ones are read a bit at a time. */
const FAST_BITS = 9;

/** The order in which a block's lengths of the code-length code are written. */
const LENGTH_CODE_ORDER = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
/** The length of a copy for symbols 257 to 285, and how many bits more each reads. */
const COPY_BASE = [
  3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
  163, 195, 227, 258,
];
const COPY_EXTRA = [
  0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/** How many bits more each of the 30 distance symbols reads. */
const DISTANCE_EXTRA = [
  0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];

/** A table of prefix codes, by the canonical rule of deflate. */
interface Code {
  /** How many codes are this many bits long. */
  counts: Uint16Array;
  /** The symbols, by the length of their code and then by their value. */
  symbols: Uint16Array;
  /** For each `FAST_BITS` bits (first bit lowest): the symbol and its length, `symbol << 4 | length`; 0 for none. */
  fast: Int32Array;
}

/** The code of these lengths (0: no code); undefined when the lengths have more codes than room. */
function codeOf(lengths: Uint8Array): Code | undefined {
  const counts = new Uint16Array(MAX_BITS + 1);
  for (const length of lengths) counts[length] = (counts[length] ?? 0) + 1;
  counts[0] = 0;
  let left = 1;
  for (let length = 1; length <= MAX_BITS; length += 1) {
    left = 2 * left - (counts[length] ?? 0);
    if (left < 0) return undefined;
  }
  const offsets = new Uint16Array(MAX_BITS + 2);
  const next = new Uint16Array(MAX_BITS + 2);
  let code = 0;
  for (let length = 1; length <= MAX_BITS; length += 1) {
    offsets[length + 1] = (offsets[length] ?? 0) + (counts[length] ?? 0);
    code = (code + (counts[length - 1] ?? 0)) << 1;
    next[length] = code;
  }
  const symbols = new Uint16Array(offsets[MAX_BITS + 1] ?? 0);
  const fast = new Int32Array(1 << FAST_BITS);
  lengths.forEach((length, symbol) => {
    if (length === 0) return;
    const at = offsets[length] ?? 0;
    offsets[length] = at + 1;
    symbols[at] = symbol;
    const value = next[length] ?? 0;
    next[length] = value + 1;
    if (length > FAST_BITS) return;
    // The code is read first bit first: the table is indexed by its bits reversed.
    let reversed = 0;
    for (let bit = 0; bit < length; bit += 1)
      reversed |= ((value >> bit) & 1) << (length - 1 - bit);
    for (let fill = reversed; fill < fast.length; fill += 1 << length) {
      fast[fill] = (symbol << 4) | length;
    }
  });
  return { counts, symbols, fast };
}

/** The bits of the stream, first bit of each byte first. */
class Bits {
  readonly #data: Uint8Array;
  #at: number;
  /** The bits read from the stream and not yet taken, `#held` of them. */
  #buffer = 0;
  #held = 0;

  constructor(data: Uint8Array, from: number) {
    this.#data = data;
    this.#at = from;
  }

  /** Holds at least `count` bits (up to 24), or as many as are left. */
  #fill(count: number): void {
    while (this.#held < count && this.#at < this.#data.length) {
      this.#buffer |= (this.#data[this.#at] ?? 0) << this.#held;
      this.#at += 1;
      this.#held += 8;
    }
  }

  /** The next `count` bits (up to 16), as a number. */
  take(count: number): number {
    if (count === 0) return 0;
    this.#fill(count);
    if (this.#held < count) throw END;
    const value = this.#buffer & ((1 << count) - 1);
    this.#buffer >>>= count;
    this.#held -= count;
    return value;
  }

  /**
   * Passes over the rest of the byte: a stored block starts on the next. The
   * whole bytes held ahead (a lookup holds up to two) are given back.
   */
  align(): void {
    this.#at -= this.#held >> 3;
    this.#buffer = 0;
    this.#held = 0;
  }

  /** Passes over up to `count` bytes (after `align`); how many there were. */
  skip(count: number): number {
    const skipped = Math.min(count, this.#data.length - this.#at);
    this.#at += skipped;
    return skipped;
  }

  /** The next symbol of `code`. */
  decode(code: Code): number {
    this.#fill(FAST_BITS);
    const entry = code.fast[this.#buffer & ((1 << FAST_BITS) - 1)] ?? 0;
    const length = entry & 15;
    if (entry !== 0 && length <= this.#held) {
      this.#buffer >>>= length;
      this.#held -= length;
      return entry >> 4;
    }
    // Longer than the table, or near the end: a bit at a time, by the canonical rule.
    let value = 0;
    let first = 0;
    let index = 0;
    for (let bits = 1; bits <= MAX_BITS; bits += 1) {
      value |= this.take(1);
      const count = code.counts[bits] ?? 0;
      if (value - first < count) return code.symbols[index + value - first] ?? 0;
      index += count;
      first = (first + count) << 1;
      value <<= 1;
    }
    throw END;
  }
}

let fixedCodes: [Code, Code] | undefined;

/** The codes of a block of fixed codes. */
function fixed(): [Code, Code] {
  if (!fixedCodes) {
    const literals = new Uint8Array(288);
    literals.fill(8, 0, 144).fill(9, 144, 256).fill(7, 256, 280).fill(8, 280, 288);
    const literalCode = codeOf(literals);
    const distanceCode = codeOf(new Uint8Array(30).fill(5));
    if (!literalCode || !distanceCode) throw new Error('the fixed codes are full');
    fixedCodes = [literalCode, distanceCode];
  }
  return fixedCodes;
}

/** The codes a block of its own codes describes at its start. */
function dynamic(bits: Bits): [Code, Code] {
  const literalCount = bits.take(5) + 257;
  const distanceCount = bits.take(5) + 1;
  const lengthCodeCount = bits.take(4) + 4;
  const lengthCodeLengths = new Uint8Array(19);
  for (let index = 0; index < lengthCodeCount; index += 1) {
    lengthCodeLengths[LENGTH_CODE_ORDER[index] ?? 0] = bits.take(3);
  }
  const lengthCode = codeOf(lengthCodeLengths);
  if (!lengthCode) throw END;
  const lengths = new Uint8Array(literalCount + distanceCount);
  for (let index = 0; index < lengths.length;) {
    const symbol = bits.decode(lengthCode);
    if (symbol < 16) {
      lengths[index] = symbol;
      index += 1;
      continue;
    }
    // A repeat: of the length before (zlib refuses one with none before), or of none.
    const value = symbol === 16 ? (lengths[index - 1] ?? 0) : 0;
    const times =
      symbol === 16 ? 3 + bits.take(2) : symbol === 17 ? 3 + bits.take(3) : 11 + bits.take(7);
    // Past the lengths there are room for, zlib refuses it; here the rest is filled.
    const end = Math.min(lengths.length, index + times);
    lengths.fill(value, index, end);
    index = end;
  }
  const literalCode = codeOf(lengths.subarray(0, literalCount));
  const distanceCode = codeOf(lengths.subarray(literalCount));
  if (!literalCode || !distanceCode) throw END;
  return [literalCode, distanceCode];
}

/**
 * What the deflated stream that starts at `from` unpacks to, in bytes, read
 * as far as its last block (or as far as it goes); counted as far as past
 * `room`.
 */
export function deflatedSize(data: Uint8Array, from: number, room: number): number {
  const bits = new Bits(data, from);
  let size = 0;
  try {
    for (let last = 0; last === 0 && size <= room;) {
      last = bits.take(1);
      const type = bits.take(2);
      if (type === 3) break;
      if (type === 0) {
        bits.align();
        const length = bits.take(16);
        // Its complement, which zlib checks; whatever it says, the block is read on.
        bits.take(16);
        const copied = bits.skip(length);
        size += copied;
        if (copied < length) break;
        continue;
      }
      const [literals, distances] = type === 1 ? fixed() : dynamic(bits);
      for (;;) {
        const symbol = bits.decode(literals);
        if (symbol < 256) {
          size += 1;
        } else if (symbol === 256) {
          break;
        } else {
          const at = symbol - 257;
          if (at >= COPY_BASE.length) throw END;
          const length = (COPY_BASE[at] ?? 0) + bits.take(COPY_EXTRA[at] ?? 0);
          const distance = bits.decode(distances);
          if (distance >= DISTANCE_EXTRA.length) throw END;
          bits.take(DISTANCE_EXTRA[distance] ?? 0);
          size += length;
        }
        if (size > room) return size;
      }
    }
  } catch (error) {
    if (error !== END) throw error;
  }
  return size;
}
