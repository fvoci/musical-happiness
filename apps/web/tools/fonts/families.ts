/**
 * The font families of the web app and how each is cut into files.
 *
 * `local` lists the names under which a copy installed on the reader's device
 * can be found, for one weight: the full name and the PostScript name of the
 * family as distributed for the web, and those of the "CJK" packaging that
 * Linux distributions and Android ship. A device that has one of them never
 * downloads the file.
 */
import type { BlockName, CharsetSpec } from './charsets.ts';

export interface FaceWeight {
  /** The `font-weight` descriptor of the rule: one weight, or a range such as `100 599`. */
  weight: string;
  local: readonly string[];
}

export interface FaceSource {
  /** Key in sources.json, and the prefix of the output file names. */
  source: string;
  /**
   * The rules written for each file of this source. A variable source serves
   * all of them from one file.
   *
   * Every file of a family has to carry the same set of descriptors. A
   * browser first picks the rules of a family whose descriptors fit the text
   * best and only then looks at `unicode-range`; a rule with another
   * `font-weight` is out of the race by then, even when it is the only one
   * that has the character.
   */
  weights: readonly FaceWeight[];
}

export interface Family {
  id: string;
  /** The CSS `font-family` name. */
  family: string;
  /**
   * `stack` families are built by default: the tokens in styles/app.css name
   * them. `optional` families are only built when asked for by id.
   */
  use: 'stack' | 'optional';
  sources: readonly FaceSource[];
  /** Variation axes to fix at one value; the others stay variable. */
  pinAxes?: Readonly<Record<string, number>>;
  common: CharsetSpec;
  /**
   * Source whose characters are left out of both files of this family. The
   * Latin family comes first in the font stack, so a CJK family is never asked
   * for a character the Latin one has.
   */
  excludeCoverageOf?: string;
}

/**
 * The two rules of a variable Noto family.
 *
 * The file from the server has every weight, so two ranges that meet at 600
 * cover the scale without a gap. They are two rules, not one, for the sake of
 * a device that has the family installed as separate files per weight: a
 * single rule could only name the regular one, and bold text would be a
 * smeared regular. With two, text from 600 up is drawn with the installed
 * bold. `cjkRegion` adds the names of the Noto CJK packaging.
 */
function notoWeights(family: string, cjkRegion?: string): FaceWeight[] {
  const names = (style: 'Regular' | 'Bold'): string[] => {
    const local = [`${family} ${style}`, `${family.replaceAll(' ', '')}-${style}`];
    if (style === 'Regular') local.unshift(family);
    if (cjkRegion) {
      const cjk = family.replace(/ (KR|JP|SC|TC)$/, ' CJK $1');
      local.push(`${cjk} ${style}`, `${cjk.replaceAll(' ', '').slice(0, -2)}${cjkRegion}-${style}`);
    }
    return local;
  };
  return [
    { weight: '100 599', local: names('Regular') },
    { weight: '600 900', local: names('Bold') },
  ];
}

const LATIN_COMMON: CharsetSpec = {
  blocks: [
    'basicLatin',
    'latin1',
    'latinExtendedA',
    'spacingModifiers',
    'combiningDiacritics',
    'generalPunctuation',
    'superscripts',
    'currency',
    'letterlike',
    'arrows',
    'mathOperators',
    'geometricShapes',
  ],
};

const CJK_SHARED: readonly BlockName[] = ['cjkPunctuation', 'fullwidthForms'];

/*
 * Each language gets its script, the everyday Han characters of its national
 * list, and the symbols of its national character set: the punctuation,
 * units, arrows and shapes that ordinary text in that language uses.
 */
const KOREAN: CharsetSpec = {
  // Every modern Hangul syllable, not only the 2,350 of KS X 1001: names and
  // loanwords use the others, and a missing one would pull in the whole remainder.
  blocks: [...CJK_SHARED, 'hangulSyllables', 'hangulCompatibilityJamo', 'enclosedCjk'],
  lists: ['ks-x-1001-symbols'],
};
const JAPANESE: CharsetSpec = {
  blocks: [...CJK_SHARED, 'hiragana', 'katakana'],
  lists: ['joyo', 'jis-x-0208-symbols'],
};
const SIMPLIFIED_CHINESE: CharsetSpec = {
  blocks: CJK_SHARED,
  lists: ['tgh-level1', 'gb-2312-symbols'],
};
const TRADITIONAL_CHINESE: CharsetSpec = {
  blocks: [...CJK_SHARED, 'bopomofo'],
  lists: ['big5-level1', 'big5-symbols'],
};

function noto(style: 'Sans' | 'Serif', use: Family['use']): Family[] {
  const base = `Noto ${style}`;
  const id = `noto-${style.toLowerCase()}`;
  const cjk = (region: string, common: CharsetSpec): Family => ({
    id: `${id}-${region}`,
    family: `${base} ${region.toUpperCase()}`,
    use,
    sources: [
      {
        source: `${id}-${region}`,
        weights: notoWeights(`${base} ${region.toUpperCase()}`, region),
      },
    ],
    common,
    excludeCoverageOf: id,
  });
  return [
    {
      id,
      family: base,
      use,
      sources: [{ source: id, weights: notoWeights(base) }],
      // The source also varies in width; the interface only uses the normal one.
      pinAxes: { wdth: 100 },
      common: LATIN_COMMON,
    },
    cjk('kr', KOREAN),
    cjk('jp', JAPANESE),
    cjk('sc', SIMPLIFIED_CHINESE),
    cjk('tc', TRADITIONAL_CHINESE),
  ];
}

export const FAMILIES: readonly Family[] = [
  ...noto('Sans', 'stack'),
  // Where a serif face is used has not been decided. The tool can build these
  // (`--family noto-serif,noto-serif-kr`), but no token names them.
  ...noto('Serif', 'optional'),
  {
    id: 'd2coding',
    family: 'D2Coding',
    use: 'stack',
    // D2Coding has no variable build: one static file per weight.
    sources: [
      {
        source: 'd2coding-regular',
        weights: [{ weight: '400', local: ['D2Coding', 'D2Coding Regular'] }],
      },
      {
        source: 'd2coding-bold',
        weights: [{ weight: '700', local: ['D2Coding Bold', 'D2Coding-Bold'] }],
      },
    ],
    common: {
      blocks: [
        'basicLatin',
        'latin1',
        'generalPunctuation',
        'arrows',
        'mathOperators',
        'boxDrawing',
        'geometricShapes',
      ],
    },
  },
];
