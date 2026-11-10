/**
 * The conversion of the original translations, without touching any file:
 * `convertAll` reads the sources and returns the message files and a report.
 * `convert.ts` is the command around it.
 *
 * This is the conversion where the original is there. Where it is not, the
 * message files written here earlier stand in for it (standalone.ts), and
 * the other sources are read the same way.
 *
 * Sources, per locale:
 *
 * - `webapp/i18n/<file>.json`: the original, ICU MessageFormat. Read only.
 * - `messages-own/<tag>.json` and `messages-own/<group>/<tag>.json`: the
 *   messages only this app has, already in the inlang format. They are copied
 *   as they are and come first: the file at the top, then the groups in the
 *   order of their names. A group is a folder so that screens built side by
 *   side each add their keys to a file of their own.
 * - `messages-fill/<tag>.json`: translations for ids the original file of
 *   that locale lacks, in ICU MessageFormat like the original. They go
 *   through the same conversion.
 * - `messages-override/<tag>.json`: texts written by hand that replace a text
 *   the original file of that locale has, in ICU MessageFormat as well. Each
 *   entry quotes the text it replaces (`was`), and the conversion stops when
 *   the original no longer says that. See `Override`.
 *
 * The result, `messages/<tag>.json`, is generated: an edit there is undone by
 * the next run. Ids are kept as they are, in the order of the English file.
 *
 * Left out are the ids listed in `removed.json` (features that are not
 * ported) and the ids no source file of the original uses.
 */
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { IcuSyntaxError, parseIcu } from './icu.ts';
import { type InlangMessage, NotConvertible, type Shape, shapeOf, toInlang } from './inlang.ts';
import {
  BASE_LOCALE,
  bundledFiles,
  LOCALES,
  type Locale,
  ORIGINAL_ROOT,
  REPOSITORY_ROOT,
  WEB_ROOT,
} from './locales.ts';
import removedIds from './removed.json' with { type: 'json' };
import { scanUsage } from './usage.ts';

export const PRODUCT_NAME = 'fvoci';

// The two names the product was known by, put together from halves like in
// tools/names/check.ts, so that this file passes that check.
const OLD_PRODUCT = ['focal', 'board'].join('');
const OLD_COMPANY = ['matter', 'most'].join('');
/** The company's name as a translation spells it in its own script. */
const OLD_COMPANY_SPELLINGS: Record<string, string[]> = { ko: [['메터', '머스트'].join('')] };
/**
 * Messages in which the company's name stands for the users of the product.
 * There it is replaced like the product's name. Anywhere else it stops the
 * conversion: such a message needs a decision, not a replacement.
 */
const COMPANY_MEANS_PRODUCT = new Set(['OnboardingTour.AddComments.Body']);

export const OUTSIDE_URL = /https?:\/\/|\bwww\.|\b[a-z0-9-]+\.(?:com|org|net|io|html)\b/i;
/**
 * What a key of this app's own messages looks like. No id of the original
 * looks like it (the conversion stops at one that does), so that the two
 * kinds can be told apart in a message file without the original.
 */
export const OWN_KEY = /^[a-z][a-z0-9_]*$/;

/** Replaces the old names in a message text. */
export function renameProduct(text: string, locale: string, id: string): string {
  const company = [OLD_COMPANY, ...(OLD_COMPANY_SPELLINGS[locale] ?? [])];
  const found = company.filter((name) => text.toLowerCase().includes(name.toLowerCase()));
  if (found.length > 0 && !COMPANY_MEANS_PRODUCT.has(id)) {
    throw new Error(`${locale} ${id}: names the old company; decide what the message should say`);
  }
  return text.replace(new RegExp([OLD_PRODUCT, ...found].join('|'), 'gi'), PRODUCT_NAME);
}

/** What an override is for. The review of the overrides is sorted by these. */
export const OVERRIDE_KINDS = [
  'name', // the product is named, or was named something else
  'rewrite', // the message says something else now
  'typo',
  'mistranslation',
  'omission', // the translation left something out
  'term', // one word for one thing
  'form', // one form for labels: a noun phrase
  'tone', // one register for sentences
  'particle', // a particle that was wrong for some of the values put in
  'punctuation',
] as const;
export type OverrideKind = (typeof OVERRIDE_KINDS)[number];

/**
 * An entry of `messages-override/<tag>.json`: the text `now` replaces the
 * text of the original.
 *
 * `was` is the text being replaced, after the old names were replaced in it
 * (so that no override file has to spell them). When the original says
 * something else, the override was written for a text that is gone, and the
 * conversion stops instead of applying it.
 *
 * `stale`, only for English, says the message means something else now: the
 * translations of the old text are left out, and each locale shows the
 * English text until its own override file gives a new translation.
 */
export interface Override {
  was: string;
  now: string;
  kind: OverrideKind[];
  stale?: boolean;
}

export type Converted = { ok: true; message: InlangMessage } | { ok: false; reason: string };

/**
 * Converts one ICU message. With `base`, the shape of the English message,
 * a translation that asks for an argument or a tag the English one does not
 * have is refused: no caller passes a value for it, and the compiled message
 * would ask every caller for one.
 */
export function convertMessage(text: string, base?: Shape): Converted {
  try {
    const nodes = parseIcu(text);
    if (base) {
      const shape = shapeOf(nodes);
      const unknown = [
        ...[...shape.arguments].filter((name) => !base.arguments.has(name)).map((n) => `{${n}}`),
        ...[...shape.tags].filter((name) => !base.tags.has(name)).map((name) => `<${name}>`),
      ];
      if (unknown.length > 0) {
        return { ok: false, reason: `not in the English message: ${unknown.join(', ')}` };
      }
    }
    return { ok: true, message: toInlang(nodes) };
  } catch (error) {
    if (error instanceof IcuSyntaxError)
      return { ok: false, reason: `ICU syntax: ${error.message}` };
    if (error instanceof NotConvertible) return { ok: false, reason: error.message };
    throw error;
  }
}

export interface ConstructCount {
  messages: number;
  occurrences: number;
  ids: string[];
}

/** What the ICU texts of one original file are made of, all its messages counted. */
export interface Survey {
  messages: number;
  /** Messages that are text only. */
  plain: number;
  /** By construct: `argument`, `tag`, `plural`, `select`, `selectordinal`, `number`, `date`, `time`, `#`. */
  constructs: Record<string, ConstructCount>;
  /** Ids with `''`, an escaped apostrophe. */
  apostrophePairs: string[];
  /** Ids where an apostrophe quotes `{`, `}`, `<` or `>`. */
  quoted: string[];
  /** Ids with an apostrophe that is just an apostrophe. */
  apostrophes: string[];
  syntaxErrors: { id: string; reason: string }[];
  /** Ids whose arguments or tags are not those of the English message. */
  shapeDiffers: { id: string; unknown: string[]; unused: string[] }[];
}

export interface LocaleReport {
  tag: string;
  file: string;
  originalCode: string | undefined;
  /** Messages in the original file. */
  original: number;
  /** Of those: left out as belonging to a feature that is not ported, and as unused. */
  removed: number;
  dead: number;
  converted: number;
  notConverted: { id: string; reason: string }[];
  filled: string[];
  /** Ids whose text is one written by hand in place of the original's. They count as converted. */
  overridden: ({ id: string } & Override)[];
  /** Ids the original translates but that are left out: the English text was rewritten since. */
  stale: string[];
  own: number;
  /** Keys of the English file this locale ends up without; the English text is shown for them. */
  missing: string[];
  /** Ids in whose text a name was replaced. */
  renamed: string[];
  survey: Survey;
}

export interface Report {
  /** The files `webapp/src/i18n.tsx` imports. */
  bundled: string[];
  /** Ids of the English file that are kept. */
  kept: string[];
  removed: { id: string; reason: string; usedIn: string[] }[];
  dead: string[];
  locales: LocaleReport[];
}

export interface Outcome {
  /**
   * What the texts of the original were read from: the original itself, or,
   * where it is not there, the message files converted from it earlier. With
   * `messages`, the report says nothing about the original: its counts
   * (`original`, `removed`, `dead`, `notConverted`, `stale`, `renamed`, the
   * survey, `bundled`, `usedIn`) are empty.
   */
  base: 'original' | 'messages';
  /** Path under the web app → content. */
  files: Map<string, string>;
  /** Locale → id → the ICU text the message was converted from; the app's own messages are not in it. */
  texts: Map<string, Record<string, string>>;
  report: Report;
}

type Messages = Record<string, string>;

function readJson(file: string): unknown {
  return JSON.parse(readFileSync(file, 'utf8')) as unknown;
}

function readOriginal(originalRoot: string, locale: Locale): Messages {
  return readJson(path.join(originalRoot, 'i18n', `${locale.file}.json`)) as Messages;
}

export function readOptional<T>(file: string): Record<string, T> {
  return existsSync(file) ? (readJson(file) as Record<string, T>) : {};
}

/**
 * The app's own messages of one locale: `messages-own/<tag>.json`, then
 * `messages-own/<group>/<tag>.json` for each group folder in name order. A
 * key that two files define stops the conversion.
 */
export function readOwn(webRoot: string, tag: string): Record<string, InlangMessage> {
  const root = path.join(webRoot, 'messages-own');
  const merged: Record<string, InlangMessage> = {};
  const add = (file: string, name: string): void => {
    for (const [key, message] of Object.entries(readOptional<InlangMessage>(file))) {
      if (key in merged) throw new Error(`${name}: "${key}" is defined in another file as well`);
      merged[key] = message;
    }
  };
  add(path.join(root, `${tag}.json`), `messages-own/${tag}.json`);
  if (!existsSync(root)) return merged;
  const groups = readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  for (const group of groups) {
    add(path.join(root, group, `${tag}.json`), `messages-own/${group}/${tag}.json`);
  }
  return merged;
}

export function readOverrides(
  webRoot: string,
  tag: string,
  isBase: boolean,
): Record<string, Override> {
  const name = `messages-override/${tag}.json`;
  const entries = readOptional<unknown>(path.join(webRoot, name));
  const kinds: readonly unknown[] = OVERRIDE_KINDS;
  for (const [id, entry] of Object.entries(entries)) {
    const fail = (problem: string): never => {
      throw new Error(`${name}: ${id}: ${problem}`);
    };
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      fail('has to be an object with "was", "now" and "kind"');
    }
    const { was, now, kind, stale, ...rest } = entry as Record<string, unknown>;
    if (typeof was !== 'string' || typeof now !== 'string') {
      fail('"was" and "now" have to be texts');
    }
    if (!Array.isArray(kind) || kind.length === 0 || !kind.every((one) => kinds.includes(one))) {
      fail(`"kind" has to be a list of ${OVERRIDE_KINDS.join(', ')}`);
    }
    if (stale !== undefined && (stale !== true || !isBase)) {
      fail('"stale" can only be true, and only for English');
    }
    if (Object.keys(rest).length > 0) fail(`unknown: ${Object.keys(rest).join(', ')}`);
  }
  return entries as Record<string, Override>;
}

/** Stops at a file in `messages-override/` that is not the file of a locale. */
export function checkOverrideFiles(webRoot: string): void {
  const overrideDirectory = path.join(webRoot, 'messages-override');
  if (!existsSync(overrideDirectory)) return;
  const unknown = readdirSync(overrideDirectory)
    .sort()
    .filter((name) => !LOCALES.some((locale) => `${locale.tag}.json` === name));
  if (unknown.length > 0) {
    throw new Error(`messages-override: not the file of a locale: ${unknown.join(', ')}`);
  }
}

/** The ids `removed.json` lists, each with the name of its group. */
export function removedIdsByReason(): Map<string, string> {
  const removed = new Map<string, string>();
  for (const [reason, { ids: list }] of Object.entries(removedIds)) {
    for (const id of list) {
      if (removed.has(id)) throw new Error(`removed.json: ${id} is listed twice`);
      removed.set(id, reason);
    }
  }
  return removed;
}

export const emptySurvey = (): Survey => ({
  messages: 0,
  plain: 0,
  constructs: {},
  apostrophePairs: [],
  quoted: [],
  apostrophes: [],
  syntaxErrors: [],
  shapeDiffers: [],
});

function survey(messages: Messages, shapes: Map<string, Shape>): Survey {
  const result = emptySurvey();
  for (const [id, text] of Object.entries(messages)) {
    result.messages += 1;
    if (text.includes("''")) result.apostrophePairs.push(id);
    else if (/'[{}<>]/.test(text)) result.quoted.push(id);
    else if (text.includes("'")) result.apostrophes.push(id);
    let shape: Shape;
    try {
      shape = shapeOf(parseIcu(text));
    } catch (error) {
      if (!(error instanceof IcuSyntaxError)) throw error;
      result.syntaxErrors.push({ id, reason: error.message });
      continue;
    }
    if (shape.constructs.size === 0) result.plain += 1;
    for (const [kind, occurrences] of shape.constructs) {
      const count = (result.constructs[kind] ??= { messages: 0, occurrences: 0, ids: [] });
      count.messages += 1;
      count.occurrences += occurrences;
      count.ids.push(id);
    }
    const base = shapes.get(id);
    if (!base) continue;
    const names = (of: Shape): string[] => [
      ...[...of.arguments].map((name) => `{${name}}`),
      ...[...of.tags].map((name) => `<${name}>`),
    ];
    const unknown = names(shape).filter((name) => !names(base).includes(name));
    const unused = names(base).filter((name) => !names(shape).includes(name));
    if (unknown.length > 0 || unused.length > 0) result.shapeDiffers.push({ id, unknown, unused });
  }
  return result;
}

export function convertAll(
  originalRoot: string = ORIGINAL_ROOT,
  webRoot: string = WEB_ROOT,
  repositoryRoot: string = REPOSITORY_ROOT,
): Outcome {
  const base = LOCALES.find((locale) => locale.tag === BASE_LOCALE);
  if (!base) throw new Error('the base locale is not among the locales');

  const bundled = bundledFiles(originalRoot);
  const listed = LOCALES.filter((locale) => locale.originalCode !== undefined)
    .map((locale) => locale.file)
    .sort();
  if (bundled.join() !== listed.join()) {
    throw new Error(
      `the original imports ${bundled.join(', ')}; locales.ts lists ${listed.join(', ')}`,
    );
  }

  // Which ids are kept.
  const english = readOriginal(originalRoot, base);
  const ids = Object.keys(english);
  const usage = scanUsage(ids, path.join(originalRoot, 'src'), repositoryRoot);
  const removed = removedIdsByReason();
  for (const id of removed.keys()) {
    if (!(id in english)) throw new Error(`removed.json: ${id} is not in the English file`);
    if (usage.get(id)?.length === 0) {
      throw new Error(`removed.json: ${id} is used nowhere; it is left out as dead already`);
    }
  }
  const dead = new Set(ids.filter((id) => usage.get(id)?.length === 0));
  const kept = ids.filter((id) => !removed.has(id) && !dead.has(id));
  const ownLooking = kept.filter((id) => OWN_KEY.test(id));
  if (ownLooking.length > 0) {
    throw new Error(
      `ids of the original that look like keys of this app's own: ${ownLooking.join(', ')}`,
    );
  }

  // The English messages come first: translations are held against their shape.
  const shapes = new Map<string, Shape>();
  for (const [id, text] of Object.entries(english)) {
    try {
      shapes.set(id, shapeOf(parseIcu(text)));
    } catch (error) {
      if (!(error instanceof IcuSyntaxError)) throw error;
    }
  }

  // Translations are held against the English text as it is shown, which is
  // the override where there is one.
  checkOverrideFiles(webRoot);
  const baseOverrides = readOverrides(webRoot, base.tag, true);
  const shown = new Map(shapes);
  const stale = new Set<string>();
  for (const [id, override] of Object.entries(baseOverrides)) {
    if (override.stale) stale.add(id);
    try {
      shown.set(id, shapeOf(parseIcu(override.now)));
    } catch (error) {
      if (!(error instanceof IcuSyntaxError)) throw error;
      throw new Error(`messages-override/${base.tag}.json: ${id}: ICU syntax: ${error.message}`, {
        cause: error,
      });
    }
  }

  const baseOwn = readOwn(webRoot, base.tag);
  for (const key of Object.keys(baseOwn)) {
    if (!OWN_KEY.test(key)) throw new Error(`messages-own: "${key}" is not lower_snake_case`);
    if (key in english) throw new Error(`messages-own: "${key}" is an id of the original`);
  }

  const files = new Map<string, string>();
  const texts = new Map<string, Record<string, string>>();
  const reports: LocaleReport[] = [];
  for (const locale of LOCALES) {
    const original = readOriginal(originalRoot, locale);
    const stray = Object.keys(original).filter((id) => !(id in english));
    if (stray.length > 0) {
      throw new Error(`${locale.file}.json has ids the English file lacks: ${stray.join(', ')}`);
    }
    const own = readOwn(webRoot, locale.tag);
    const strayOwn = Object.keys(own).filter((key) => !(key in baseOwn));
    if (strayOwn.length > 0) {
      throw new Error(`messages-own (${locale.tag}): not in English: ${strayOwn.join(', ')}`);
    }
    const fill = readOptional<string>(path.join(webRoot, 'messages-fill', `${locale.tag}.json`));
    const strayFill = Object.keys(fill).filter((id) => !kept.includes(id));
    if (strayFill.length > 0) {
      throw new Error(`messages-fill/${locale.tag}.json: not a kept id: ${strayFill.join(', ')}`);
    }

    const overrides = locale === base ? baseOverrides : readOverrides(webRoot, locale.tag, false);
    const overrideFile = `messages-override/${locale.tag}.json`;
    const strayOverride = Object.keys(overrides).filter((id) => !kept.includes(id));
    if (strayOverride.length > 0) {
      throw new Error(`${overrideFile}: not a kept id: ${strayOverride.join(', ')}`);
    }

    const report: LocaleReport = {
      tag: locale.tag,
      file: locale.file,
      originalCode: locale.originalCode,
      original: Object.keys(original).length,
      removed: Object.keys(original).filter((id) => removed.has(id)).length,
      dead: Object.keys(original).filter((id) => dead.has(id)).length,
      converted: 0,
      notConverted: [],
      filled: [],
      overridden: [],
      stale: [],
      own: 0,
      missing: [],
      renamed: [],
      survey: survey(original, locale === base ? new Map<string, Shape>() : shapes),
    };
    const output: Record<string, InlangMessage> = {};
    const sources: Record<string, string> = {};
    for (const key of Object.keys(baseOwn)) {
      const message = own[key];
      if (message === undefined) {
        report.missing.push(key);
        continue;
      }
      output[key] = message;
      report.own += 1;
    }
    for (const id of kept) {
      const shape = locale === base ? undefined : shown.get(id);
      /** The text with the old names replaced, and what it converts to. */
      const convert = (text: string): Converted & { text: string } => {
        if (OUTSIDE_URL.test(text)) {
          return { text, ok: false, reason: 'the text has the address of an outside site' };
        }
        const renamed = renameProduct(text, locale.tag, id);
        return { text: renamed, ...convertMessage(renamed, shape) };
      };
      const emit = (text: string, message: InlangMessage): void => {
        output[id] = message;
        sources[id] = text;
      };
      const fromOriginal = original[id] === undefined ? undefined : convert(original[id]);
      if (fromOriginal && fromOriginal.text !== original[id]) report.renamed.push(id);

      const override = overrides[id];
      if (override) {
        const refuse = (problem: string): Error => new Error(`${overrideFile}: ${id}: ${problem}`);
        if (!fromOriginal) {
          throw refuse('the original has no text to replace; a new text belongs in messages-fill');
        }
        if (override.was !== fromOriginal.text) {
          throw refuse(`"was" is not what the original says: ${JSON.stringify(fromOriginal.text)}`);
        }
        if (override.now === override.was) throw refuse('"now" is the text of the original');
        // A text written by hand has to pass what a translation has to pass;
        // where a translation would be left out, this stops.
        const written = convert(override.now);
        if (written.text !== override.now) throw refuse('"now" has one of the old names');
        if (!written.ok) throw refuse(written.reason);
        emit(override.now, written.message);
        report.converted += 1;
        report.overridden.push({ id, ...override });
        continue;
      }

      // The English text was rewritten: what the original has translates the old one.
      if (locale !== base && stale.has(id) && fromOriginal) {
        if (id in fill) {
          throw new Error(
            `messages-fill/${locale.tag}.json: ${id} is in the original; its new translation belongs in ${overrideFile}`,
          );
        }
        report.stale.push(id);
        report.missing.push(id);
        continue;
      }

      if (fromOriginal?.ok) {
        if (id in fill) {
          throw new Error(`messages-fill/${locale.tag}.json: ${id} is in the original already`);
        }
        emit(fromOriginal.text, fromOriginal.message);
        report.converted += 1;
        continue;
      }
      if (fromOriginal) report.notConverted.push({ id, reason: fromOriginal.reason });
      const filled = fill[id] === undefined ? undefined : convert(fill[id]);
      if (filled?.ok) {
        emit(filled.text, filled.message);
        report.filled.push(id);
      } else {
        if (filled) {
          throw new Error(`messages-fill/${locale.tag}.json: ${id}: ${filled.reason}`);
        }
        if (locale === base) {
          throw new Error(`en ${id}: ${fromOriginal?.reason ?? 'no text'}`);
        }
        report.missing.push(id);
      }
    }
    texts.set(locale.tag, sources);
    files.set(`messages/${locale.tag}.json`, `${JSON.stringify(output, null, 2)}\n`);
    reports.push(report);
  }

  return {
    base: 'original',
    files,
    texts,
    report: {
      bundled,
      kept,
      removed: [...removed].map(([id, reason]) => ({ id, reason, usedIn: usage.get(id) ?? [] })),
      dead: [...dead],
      locales: reports,
    },
  };
}
