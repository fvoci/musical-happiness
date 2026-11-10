import { describe, expect, it } from 'vitest';

import { readList, resolveCharset } from '../../tools/fonts/charsets.ts';
import { FAMILIES } from '../../tools/fonts/families.ts';
import { readCodePoints } from '../../tools/fonts/sfnt.ts';
import sources from '../../tools/fonts/sources.json' with { type: 'json' };
import { formatUnicodeRange, toRanges, toText } from '../../tools/fonts/unicode-range.ts';

describe('unicode-range', () => {
  it('merges neighbours and keeps gaps', () => {
    expect(toRanges([5, 1, 2, 3, 9, 10, 3])).toEqual([
      [1, 3],
      [5, 5],
      [9, 10],
    ]);
  });

  it('writes the CSS notation', () => {
    expect(formatUnicodeRange([0xac00, 0xac01, 0xac02, 0x3131, 0x1f600])).toBe(
      'U+3131,U+AC00-AC02,U+1F600',
    );
  });

  it('turns code points outside the BMP into text', () => {
    expect(toText([0x41, 0x1f600])).toBe('A😀');
  });
});

describe('character lists', () => {
  it.each([
    ['joyo', 2136],
    ['tgh-level1', 3500],
    ['big5-level1', 5401],
  ] as const)('%s has %d characters, all of them Han', (name, count) => {
    const list = readList(name);
    expect(new Set(list).size).toBe(count);
    expect(list).toHaveLength(count);
    for (const code of list) expect(code).toBeGreaterThanOrEqual(0x3400);
  });

  it.each([
    ['ks-x-1001-symbols', 986, '※'],
    ['jis-x-0208-symbols', 524, '→'],
    ['gb-2312-symbols', 728, '、'],
    ['big5-symbols', 409, '。'],
  ] as const)('%s has %d characters, next to no Han among them', (name, count, sample) => {
    const list = readList(name);
    expect(list).toHaveLength(count);
    expect(list).toContain(sample.codePointAt(0));
    // The sets do hold a few Han characters used as symbols: units and ditto marks.
    const han = list.filter((code) => code >= 0x3400 && code <= 0x9fff);
    expect(han.length).toBeLessThanOrEqual(12);
  });

  it('the Korean common set is every modern syllable plus punctuation', () => {
    const korean = FAMILIES.find((family) => family.id === 'noto-sans-kr');
    const set = resolveCharset(korean?.common ?? { blocks: [] });
    expect(set.has(0xac00)).toBe(true);
    expect(set.has(0xd7a3)).toBe(true);
    expect(set.has(0x3001)).toBe(true);
    expect(set.has(0x203b)).toBe(true); // ※, from the symbols of KS X 1001
    expect(set.has(0x41)).toBe(false);
  });
});

describe('families', () => {
  it('every source is pinned by a SHA-256 for the font and its licence', () => {
    for (const family of FAMILIES) {
      for (const { source } of family.sources) {
        const pinned = (sources.fonts as Record<string, (typeof sources.fonts)['noto-sans']>)[
          source
        ];
        expect(pinned, source).toBeDefined();
        expect(pinned?.font.sha256).toMatch(/^[0-9a-f]{64}$/);
        expect(pinned?.license.sha256).toMatch(/^[0-9a-f]{64}$/);
      }
    }
  });

  it('builds Noto Sans and D2Coding by default and Noto Serif only on request', () => {
    const byUse = (use: string): string[] =>
      FAMILIES.filter((family) => family.use === use).map((family) => family.id);
    expect(byUse('stack')).toEqual([
      'noto-sans',
      'noto-sans-kr',
      'noto-sans-jp',
      'noto-sans-sc',
      'noto-sans-tc',
      'd2coding',
    ]);
    expect(byUse('optional')).toEqual([
      'noto-serif',
      'noto-serif-kr',
      'noto-serif-jp',
      'noto-serif-sc',
      'noto-serif-tc',
    ]);
  });

  it('gives every file of a family the same weight descriptors', () => {
    // Rules of one family only combine by unicode-range when their other
    // descriptors are identical (see FaceSource.weights).
    const korean = FAMILIES.find((family) => family.id === 'noto-sans-kr');
    const weights = korean?.sources[0]?.weights ?? [];
    expect(weights.map(({ weight }) => weight)).toEqual(['100 599', '600 900']);
    expect(weights[0]?.local).toEqual([
      'Noto Sans KR',
      'Noto Sans KR Regular',
      'NotoSansKR-Regular',
      'Noto Sans CJK KR Regular',
      'NotoSansCJKkr-Regular',
    ]);
    expect(weights[1]?.local).toEqual([
      'Noto Sans KR Bold',
      'NotoSansKR-Bold',
      'Noto Sans CJK KR Bold',
      'NotoSansCJKkr-Bold',
    ]);
  });
});

/** A font file with nothing but a cmap table holding the given subtable. */
function fontWithCmap(platform: number, encoding: number, subtable: Buffer): Buffer {
  const cmapHeader = Buffer.alloc(12);
  cmapHeader.writeUInt16BE(1, 2);
  cmapHeader.writeUInt16BE(platform, 4);
  cmapHeader.writeUInt16BE(encoding, 6);
  cmapHeader.writeUInt32BE(12, 8);
  const cmap = Buffer.concat([cmapHeader, subtable]);
  const directory = Buffer.alloc(28);
  directory.writeUInt32BE(0x00010000, 0);
  directory.writeUInt16BE(1, 4);
  directory.write('cmap', 12, 'latin1');
  directory.writeUInt32BE(28, 20);
  directory.writeUInt32BE(cmap.length, 24);
  return Buffer.concat([directory, cmap]);
}

describe('character map of a font', () => {
  it('reads segmented coverage (format 12) and skips the missing glyph', () => {
    const sub = Buffer.alloc(16 + 2 * 12);
    sub.writeUInt16BE(12, 0);
    sub.writeUInt32BE(2, 12);
    // U+0041-0043 -> glyphs 0, 1, 2: the first maps to "missing".
    sub.writeUInt32BE(0x41, 16);
    sub.writeUInt32BE(0x43, 20);
    sub.writeUInt32BE(0, 24);
    // U+1F600 -> glyph 9
    sub.writeUInt32BE(0x1f600, 28);
    sub.writeUInt32BE(0x1f600, 32);
    sub.writeUInt32BE(9, 36);
    expect(readCodePoints(fontWithCmap(3, 10, sub))).toEqual([0x42, 0x43, 0x1f600]);
  });

  it('reads segment mapping to delta values (format 4)', () => {
    // Two segments: U+AC00-AC02 with delta, and the closing U+FFFF.
    const segments = 2;
    const sub = Buffer.alloc(14 + segments * 2 + 2 + segments * 6);
    sub.writeUInt16BE(4, 0);
    sub.writeUInt16BE(segments * 2, 6);
    let at = 14;
    for (const end of [0xac02, 0xffff]) at = sub.writeUInt16BE(end, at);
    at += 2;
    for (const start of [0xac00, 0xffff]) at = sub.writeUInt16BE(start, at);
    for (const delta of [(5 - 0xac00) & 0xffff, 1]) at = sub.writeUInt16BE(delta, at);
    expect(readCodePoints(fontWithCmap(3, 1, sub))).toEqual([0xac00, 0xac01, 0xac02]);
  });
});
