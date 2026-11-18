/**
 * The elements of an HWPX package's list (`content.hpf`) as rhwp (0.8.7)
 * sees them. Its reader is quick-xml 0.42 (`Reader::from_str`, the default
 * configuration), whose rules for where markup ends are its own, not XML's:
 * a quote opens a value anywhere in a tag, end tags hold quotes too, a
 * DOCTYPE holds them in places, and the attributes rhwp takes are the ones
 * quick-xml's iterator gives without an error. They are ported here as far
 * as they decide what is an element and what its attributes are, so that
 * what is counted is what rhwp reads (hwpx-package.ts).
 *
 * Where quick-xml stops with an error, rhwp refuses the file; so this throws
 * `UnreadableFile` there.
 */
import { UnreadableFile } from './renderer.ts';

/** A start or empty tag: its name as written, and the attributes rhwp takes, in order. */
export interface Tag {
  name: string;
  attributes: [string, string][];
}

const isSpace = (char: string | undefined): boolean =>
  char === ' ' || char === '\r' || char === '\n' || char === '\t';

/** Where `mark` starts at or after `from`; throws when it does not come. */
function find(xml: string, mark: string, from: number): number {
  const at = xml.indexOf(mark, from);
  if (at < 0) throw new UnreadableFile();
  return at;
}

/** The `>` that ends a tag whose text starts at `from`, a quote holding until it closes. */
function tagEnd(xml: string, from: number): number {
  let quote: string | undefined;
  for (let at = from; at < xml.length; at += 1) {
    const char = xml[at];
    if (quote !== undefined) {
      if (char === quote) quote = undefined;
    } else if (char === '>') {
      return at;
    } else if (char === '"' || char === "'") {
      quote = char;
    }
  }
  throw new UnreadableFile();
}

/** The first of `marks` at or after `from`, or -1. */
function firstOf(xml: string, from: number, marks: string): number {
  for (let at = from; at < xml.length; at += 1) {
    if (marks.includes(xml.charAt(at))) return at;
  }
  return -1;
}

/** The declarations of an internal subset in which a quote holds until it closes. */
const QUOTED_DECLARATIONS = ['!ENTITY', '!ATTLIST', '!NOTATION'];

/** Where a DOCTYPE's internal subset, from `from` (after `[`), ends: at its `]`. */
function subsetEnd(xml: string, from: number): number {
  let at = from;
  for (;;) {
    const mark = firstOf(xml, at, ']<');
    if (mark < 0) throw new UnreadableFile();
    if (xml[mark] === ']') return mark;
    const markup = mark + 1;
    if (xml[markup] === '?') {
      at = find(xml, '?>', markup + 1) + 1;
    } else if (xml.startsWith('!--', markup)) {
      at = find(xml, '-->', markup + 3) + 3;
    } else if (xml.startsWith('!ELEMENT', markup)) {
      at = find(xml, '>', markup + 8) + 1;
    } else {
      const quoted = QUOTED_DECLARATIONS.find((name) => xml.startsWith(name, markup));
      at = quoted ? tagEnd(xml, markup + quoted.length) : find(xml, '>', markup);
      at += 1;
    }
  }
}

/** The `>` that ends a DOCTYPE whose text after `<!` starts at `from`. */
function doctypeEnd(xml: string, from: number): number {
  let at = from;
  for (;;) {
    const mark = firstOf(xml, at, `'"[>`);
    if (mark < 0) throw new UnreadableFile();
    const char = xml[mark] ?? '';
    if (char === '>') return mark;
    if (char === '[') return find(xml, '>', subsetEnd(xml, mark + 1) + 1);
    at = find(xml, char, mark + 1) + 1;
  }
}

/** The `>` that ends a comment, character data or a DOCTYPE at `open`. */
function bangEnd(xml: string, open: number): number {
  if (xml.startsWith('<!--', open)) return find(xml, '-->', open + 4) + 2;
  if (xml.startsWith('<![CDATA[', open)) return find(xml, ']]>', open + 9) + 2;
  // The keyword in any case of ASCII letters (no `u` flag: no other letter matches one).
  if (!/^<!doctype/i.test(xml.slice(open, open + 9))) throw new UnreadableFile();
  const end = doctypeEnd(xml, open + 2);
  // A DOCTYPE names its root.
  if (!/[^ \r\n\t]/.test(xml.slice(open + 9, end))) throw new UnreadableFile();
  return end;
}

/** The attributes quick-xml's iterator gives without an error (the ones `.flatten()` keeps). */
export function attributesOf(content: string, from: number): [string, string][] {
  const found: [string, string][] = [];
  const keys = new Set<string>();
  const firstSpace = (at: number): number => {
    while (at < content.length && !isSpace(content[at])) at += 1;
    return at < content.length ? at : -1;
  };
  const skipSpace = (at: number): number => {
    while (at < content.length && isSpace(content[at])) at += 1;
    return at;
  };
  let offset = from;
  while (offset >= 0 && offset < content.length) {
    const start = skipSpace(offset);
    if (start >= content.length) break;
    let end = start + 1;
    while (end < content.length && content[end] !== '=' && !isSpace(content[end])) end += 1;
    if (end >= content.length) break;
    const equals = content[end] === '=' ? end : skipSpace(end + 1);
    if (equals >= content.length) break;
    if (content[equals] !== '=') {
      // A name with no value: an error, and the next name starts here.
      offset = equals;
      continue;
    }
    const key = content.slice(start, end);
    if (keys.has(key)) {
      // A name given twice: an error, and what follows is skipped to a blank.
      offset = firstSpace(equals);
      continue;
    }
    keys.add(key);
    const value = skipSpace(equals + 1);
    const quote = content[value];
    if (quote === undefined) break;
    if (quote !== '"' && quote !== "'") {
      offset = firstSpace(value);
      continue;
    }
    const close = content.indexOf(quote, value + 1);
    if (close < 0) break;
    found.push([key, content.slice(value + 1, close)]);
    offset = close + 1;
  }
  return found;
}

/** Throws where text holds an `&` with no `;` before the next `&` or its end. */
function checkReferences(text: string): void {
  if (!text.includes('&')) return;
  if (text.split('&').some((part, index) => index > 0 && !part.includes(';'))) {
    throw new UnreadableFile();
  }
}

/**
 * The start and empty tags of `xml`, in order, as rhwp's reader gives them.
 * Throws `UnreadableFile` where it stops with an error: markup that does not
 * end, an end tag that closes no element of its name, an `&` that is no
 * reference, markup it does not know.
 */
export function tagsOf(xml: string): Tag[] {
  const tags: Tag[] = [];
  const open: string[] = [];
  let at = 0;
  for (;;) {
    const start = xml.indexOf('<', at);
    checkReferences(xml.slice(at, start < 0 ? xml.length : start));
    if (start < 0) return tags;
    const next = xml[start + 1];
    let end: number;
    if (next === undefined) throw new UnreadableFile();
    if (next === '!') {
      end = bangEnd(xml, start);
    } else if (next === '?') {
      end = find(xml, '?>', start + 1) + 1;
      if (end - start < 3) throw new UnreadableFile();
    } else if (next === '/') {
      end = tagEnd(xml, start + 1);
      // Blanks after the name are let be; a name of blanks alone is kept as it is.
      const written = xml.slice(start + 2, end);
      const name = written.replace(/[ \r\n\t]+$/, '') || written;
      if (open.pop() !== name) throw new UnreadableFile();
    } else {
      end = tagEnd(xml, start + 1);
      const empty = end - 1 > start && xml[end - 1] === '/';
      const content = xml.slice(start + 1, empty ? end - 1 : end);
      let nameEnd = 0;
      while (nameEnd < content.length && !isSpace(content[nameEnd])) nameEnd += 1;
      const name = content.slice(0, nameEnd);
      if (!empty) open.push(name);
      tags.push({ name, attributes: attributesOf(content, nameEnd) });
    }
    at = end + 1;
  }
}
