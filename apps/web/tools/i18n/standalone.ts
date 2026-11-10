/**
 * The conversion where the original is not there: the message files written
 * from it earlier stand in for it.
 *
 * `messages/<tag>.json` then holds the only copy of the texts that came from
 * the original, and it is read as a source: what it says for an id of the
 * original is kept, unless one of the other sources says something else.
 * Those are read as they always were (see conversion.ts):
 *
 * - `messages-own/`: the app's own messages. They are written anew on every
 *   run, so a key that is added, changed or taken away there shows in the
 *   message files.
 * - `messages-override/<tag>.json`: `now` is the text of the id. `was` cannot
 *   be held against the original any more; it stays as the record of what
 *   was replaced.
 * - `messages-fill/<tag>.json`: the text of the id.
 * - `removed.json`: an id listed there is left out of every locale.
 *
 * So a text of the original is changed the way it was before: by an entry in
 * `messages-override/` (or `messages-fill/`, for an id the locale's original
 * lacked). An id that no such file names keeps what the message file says,
 * and an edit made there by hand stays. Such a text has to be one the
 * conversion could have written: text, `{arguments}` and `{#tags}`, with no
 * argument or tag the English message lacks.
 *
 * Run on message files the conversion from the original wrote, this returns
 * the same files, byte for byte (a test holds the two against each other
 * where the original is there).
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { hasOriginalWebapp } from '../original.ts';

import {
  checkOverrideFiles,
  convertAll,
  type Converted,
  convertMessage,
  emptySurvey,
  type LocaleReport,
  type Outcome,
  OUTSIDE_URL,
  OWN_KEY,
  readOptional,
  readOverrides,
  readOwn,
  removedIdsByReason,
  renameProduct,
} from './conversion.ts';
import { IcuSyntaxError, parseIcu } from './icu.ts';
import { type InlangMessage, type Shape, shapeOf, toInlang } from './inlang.ts';
import { BASE_LOCALE, LOCALES, ORIGINAL_ROOT, REPOSITORY_ROOT, WEB_ROOT } from './locales.ts';

/** A message of a message file that is not one the conversion writes for text, arguments and tags. */
export class NotReadable extends Error {
  constructor(reason: string) {
    super(reason);
    this.name = 'NotReadable';
  }
}

const PIECE =
  /\\([\\{}])|\{#([A-Za-z][A-Za-z0-9]*)\}|\{\/([A-Za-z][A-Za-z0-9]*)\}|\{([A-Za-z_][A-Za-z0-9_]*)\}|([^\\{}]+)|([\\{}])/g;

/**
 * The ICU text a message of a message file was converted from: the way back
 * of `toInlang`, for what the translations of the original are made of (text,
 * arguments, tags). The result is converted again and has to give the very
 * message; anything else is not readable.
 */
export function icuTextOf(message: InlangMessage): string {
  if (typeof message !== 'string') throw new NotReadable('a message with variants');
  let text = '';
  for (const [, escaped, opened, closed, argument, plain, stray] of message.matchAll(PIECE)) {
    if (escaped !== undefined) text += escaped === '\\' ? '\\' : `'${escaped}'`;
    else if (opened !== undefined) text += `<${opened}>`;
    else if (closed !== undefined) text += `</${closed}>`;
    else if (argument !== undefined) text += `{${argument}}`;
    else if (plain !== undefined) text += plain;
    else throw new NotReadable(`"${stray ?? ''}" where a pattern has to escape it`);
  }
  let again: InlangMessage;
  try {
    again = toInlang(parseIcu(text));
  } catch (error) {
    throw new NotReadable(error instanceof Error ? error.message : String(error));
  }
  if (again !== message) throw new NotReadable('it does not convert back to itself');
  return text;
}

function readMessages(webRoot: string, tag: string): Record<string, InlangMessage> {
  const file = path.join(webRoot, 'messages', `${tag}.json`);
  if (!existsSync(file)) {
    throw new Error(
      `messages/${tag}.json is missing: without the original it is the source of the texts of the locale`,
    );
  }
  return JSON.parse(readFileSync(file, 'utf8')) as Record<string, InlangMessage>;
}

/**
 * `removed` is what `removed.json` lists: id → the name of its group. An id
 * listed there after the original went is left out from then on.
 */
export function convertStandalone(
  webRoot: string = WEB_ROOT,
  removed: ReadonlyMap<string, string> = removedIdsByReason(),
): Outcome {
  const base = LOCALES.find((locale) => locale.tag === BASE_LOCALE);
  if (!base) throw new Error('the base locale is not among the locales');

  // Which ids are kept: those of the original the English file has, less the removed ones.
  const english = readMessages(webRoot, base.tag);
  const inherited = Object.keys(english).filter((key) => !OWN_KEY.test(key));
  const kept = inherited.filter((id) => !removed.has(id));

  /** The ICU text of a message kept from a message file, or the reason it cannot be kept. */
  const read = (tag: string, id: string, message: InlangMessage): string => {
    try {
      return icuTextOf(message);
    } catch (error) {
      if (!(error instanceof NotReadable)) throw error;
      throw new Error(`messages/${tag}.json: ${id}: ${error.message}`, { cause: error });
    }
  };

  // Translations are held against the English text as it is shown.
  checkOverrideFiles(webRoot);
  const baseOverrides = readOverrides(webRoot, base.tag, true);
  const shown = new Map<string, Shape>();
  const stale = new Set<string>();
  for (const id of kept) {
    const override = baseOverrides[id];
    if (override?.stale) stale.add(id);
    const message = english[id];
    if (message === undefined) continue;
    try {
      shown.set(id, shapeOf(parseIcu(override ? override.now : read(base.tag, id, message))));
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
  }

  const files = new Map<string, string>();
  const texts = new Map<string, Record<string, string>>();
  const reports: LocaleReport[] = [];
  for (const locale of LOCALES) {
    const existing = locale === base ? english : readMessages(webRoot, locale.tag);
    const stray = Object.keys(existing).filter((key) => !OWN_KEY.test(key) && !(key in english));
    if (stray.length > 0) {
      throw new Error(
        `messages/${locale.tag}.json has ids the English file lacks: ${stray.join(', ')}`,
      );
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
      original: 0,
      removed: 0,
      dead: 0,
      converted: 0,
      notConverted: [],
      filled: [],
      overridden: [],
      stale: [],
      own: 0,
      missing: [],
      renamed: [],
      survey: emptySurvey(),
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
      /** A text held to what a text of the original is held to, and what it converts to. */
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

      const override = overrides[id];
      if (override) {
        const refuse = (problem: string): Error => new Error(`${overrideFile}: ${id}: ${problem}`);
        if (override.now === override.was) throw refuse('"now" is the text of the original');
        const written = convert(override.now);
        if (written.text !== override.now) throw refuse('"now" has one of the old names');
        if (!written.ok) throw refuse(written.reason);
        emit(override.now, written.message);
        report.converted += 1;
        report.overridden.push({ id, ...override });
        continue;
      }

      const filling = fill[id];
      if (filling !== undefined) {
        const filled = convert(filling);
        if (!filled.ok) {
          throw new Error(`messages-fill/${locale.tag}.json: ${id}: ${filled.reason}`);
        }
        emit(filled.text, filled.message);
        report.filled.push(id);
        continue;
      }

      // The English text was rewritten: what the file has translates the old one.
      const message = existing[id];
      if (message === undefined || (locale !== base && stale.has(id))) {
        report.missing.push(id);
        continue;
      }
      // What the file says stays, held to what a text of the original is held to.
      const text = read(locale.tag, id, message);
      const held = convert(text);
      const problem = !held.ok
        ? held.reason
        : held.text !== text
          ? 'the text has one of the old names'
          : undefined;
      if (problem !== undefined) {
        throw new Error(`messages/${locale.tag}.json: ${id}: ${problem}`);
      }
      emit(text, message);
      report.converted += 1;
    }
    texts.set(locale.tag, sources);
    files.set(`messages/${locale.tag}.json`, `${JSON.stringify(output, null, 2)}\n`);
    reports.push(report);
  }

  return {
    base: 'messages',
    files,
    texts,
    report: {
      bundled: [],
      kept,
      removed: [...removed].map(([id, reason]) => ({ id, reason, usedIn: [] })),
      dead: [],
      locales: reports,
    },
  };
}

/**
 * The conversion this checkout can do: from the original where it is there,
 * from the message files where it is not.
 */
export function convertAvailable(
  originalRoot: string = ORIGINAL_ROOT,
  webRoot: string = WEB_ROOT,
  repositoryRoot: string = REPOSITORY_ROOT,
): Outcome {
  return hasOriginalWebapp(originalRoot)
    ? convertAll(originalRoot, webRoot, repositoryRoot)
    : convertStandalone(webRoot);
}
