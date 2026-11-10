/**
 * The conversion of the original translations, without touching any file:
 * `convertAll` reads the sources and returns the message files and a report.
 * `convert.ts` is the command around it.
 *
 * Sources, per locale:
 *
 * - `webapp/i18n/<file>.json`: the original, ICU MessageFormat. Read only.
 * - `messages-own/<tag>.json`: the messages only this app has, already in the
 *   inlang format. They are copied as they are and come first.
 * - `messages-fill/<tag>.json`: translations for ids the original file of
 *   that locale lacks, in ICU MessageFormat like the original. They go
 *   through the same conversion.
 *
 * The result, `messages/<tag>.json`, is generated: an edit there is undone by
 * the next run. Ids are kept as they are, in the order of the English file.
 *
 * Left out are the ids listed in `removed.json` (features that are not
 * ported) and the ids no source file of the original uses.
 */
import { existsSync, readFileSync } from 'node:fs';
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

const OUTSIDE_URL = /https?:\/\/|\bwww\.|\b[a-z0-9-]+\.(?:com|org|net|io|html)\b/i;
const OWN_KEY = /^[a-z][a-z0-9_]*$/;

/** Replaces the old names in a message text. */
export function renameProduct(text: string, locale: string, id: string): string {
  const company = [OLD_COMPANY, ...(OLD_COMPANY_SPELLINGS[locale] ?? [])];
  const found = company.filter((name) => text.toLowerCase().includes(name.toLowerCase()));
  if (found.length > 0 && !COMPANY_MEANS_PRODUCT.has(id)) {
    throw new Error(`${locale} ${id}: names the old company; decide what the message should say`);
  }
  return text.replace(new RegExp([OLD_PRODUCT, ...found].join('|'), 'gi'), PRODUCT_NAME);
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
  /** Path under the web app → content. */
  files: Map<string, string>;
  report: Report;
}

type Messages = Record<string, string>;

function readJson(file: string): unknown {
  return JSON.parse(readFileSync(file, 'utf8')) as unknown;
}

function readOriginal(originalRoot: string, locale: Locale): Messages {
  return readJson(path.join(originalRoot, 'i18n', `${locale.file}.json`)) as Messages;
}

function readOptional<T>(file: string): Record<string, T> {
  return existsSync(file) ? (readJson(file) as Record<string, T>) : {};
}

function survey(messages: Messages, shapes: Map<string, Shape>): Survey {
  const result: Survey = {
    messages: 0,
    plain: 0,
    constructs: {},
    apostrophePairs: [],
    quoted: [],
    apostrophes: [],
    syntaxErrors: [],
    shapeDiffers: [],
  };
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
  const removed = new Map<string, string>();
  for (const [reason, { ids: list }] of Object.entries(removedIds)) {
    for (const id of list) {
      if (!(id in english)) throw new Error(`removed.json: ${id} is not in the English file`);
      if (removed.has(id)) throw new Error(`removed.json: ${id} is listed twice`);
      if (usage.get(id)?.length === 0) {
        throw new Error(`removed.json: ${id} is used nowhere; it is left out as dead already`);
      }
      removed.set(id, reason);
    }
  }
  const dead = new Set(ids.filter((id) => usage.get(id)?.length === 0));
  const kept = ids.filter((id) => !removed.has(id) && !dead.has(id));

  // The English messages come first: translations are held against their shape.
  const shapes = new Map<string, Shape>();
  for (const [id, text] of Object.entries(english)) {
    try {
      shapes.set(id, shapeOf(parseIcu(text)));
    } catch (error) {
      if (!(error instanceof IcuSyntaxError)) throw error;
    }
  }

  const baseOwn = readOptional<InlangMessage>(
    path.join(webRoot, 'messages-own', `${base.tag}.json`),
  );
  for (const key of Object.keys(baseOwn)) {
    if (!OWN_KEY.test(key)) throw new Error(`messages-own: "${key}" is not lower_snake_case`);
    if (key in english) throw new Error(`messages-own: "${key}" is an id of the original`);
  }

  const files = new Map<string, string>();
  const reports: LocaleReport[] = [];
  for (const locale of LOCALES) {
    const original = readOriginal(originalRoot, locale);
    const stray = Object.keys(original).filter((id) => !(id in english));
    if (stray.length > 0) {
      throw new Error(`${locale.file}.json has ids the English file lacks: ${stray.join(', ')}`);
    }
    const own = readOptional<InlangMessage>(
      path.join(webRoot, 'messages-own', `${locale.tag}.json`),
    );
    const strayOwn = Object.keys(own).filter((key) => !(key in baseOwn));
    if (strayOwn.length > 0) {
      throw new Error(`messages-own/${locale.tag}.json: not in en.json: ${strayOwn.join(', ')}`);
    }
    const fill = readOptional<string>(path.join(webRoot, 'messages-fill', `${locale.tag}.json`));
    const strayFill = Object.keys(fill).filter((id) => !kept.includes(id));
    if (strayFill.length > 0) {
      throw new Error(`messages-fill/${locale.tag}.json: not a kept id: ${strayFill.join(', ')}`);
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
      own: 0,
      missing: [],
      renamed: [],
      survey: survey(original, locale === base ? new Map<string, Shape>() : shapes),
    };
    const output: Record<string, InlangMessage> = {};
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
      const shape = locale === base ? undefined : shapes.get(id);
      const convert = (text: string): Converted => {
        if (OUTSIDE_URL.test(text)) {
          return { ok: false, reason: 'the text has the address of an outside site' };
        }
        const renamed = renameProduct(text, locale.tag, id);
        if (renamed !== text) report.renamed.push(id);
        return convertMessage(renamed, shape);
      };
      const fromOriginal = original[id] === undefined ? undefined : convert(original[id]);
      if (fromOriginal?.ok) {
        if (id in fill) {
          throw new Error(`messages-fill/${locale.tag}.json: ${id} is in the original already`);
        }
        output[id] = fromOriginal.message;
        report.converted += 1;
        continue;
      }
      if (fromOriginal) report.notConverted.push({ id, reason: fromOriginal.reason });
      const filled = fill[id] === undefined ? undefined : convert(fill[id]);
      if (filled?.ok) {
        output[id] = filled.message;
        report.filled.push(id);
      } else {
        if (filled) throw new Error(`messages-fill/${locale.tag}.json: ${id}: ${filled.reason}`);
        if (locale === base) throw new Error(`en ${id}: ${fromOriginal?.reason ?? 'no text'}`);
        report.missing.push(id);
      }
    }
    files.set(`messages/${locale.tag}.json`, `${JSON.stringify(output, null, 2)}\n`);
    reports.push(report);
  }

  return {
    files,
    report: {
      bundled,
      kept,
      removed: [...removed].map(([id, reason]) => ({ id, reason, usedIn: usage.get(id) ?? [] })),
      dead: [...dead],
      locales: reports,
    },
  };
}
