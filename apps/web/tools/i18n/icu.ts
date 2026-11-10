/**
 * A parser for ICU MessageFormat as the original web app read it.
 *
 * The original formatted its messages with react-intl 5, whose parser has a
 * few rules of its own, and those are followed here so that a text counts as
 * broken exactly when the original could not format it:
 *
 * - An apostrophe quotes only when `{`, `}`, `<` or `>` follows it (or `#`
 *   inside a plural). `''` is one apostrophe. Any other apostrophe is itself.
 * - `<name>…</name>` is a tag. `<name/>` and a `<` that is not followed by a
 *   letter or `/` are text. A closing tag without its opening tag is an error.
 * - `}` outside of any argument is text.
 * - `#` stands for the number only directly inside a plural, not inside a
 *   select nested in it.
 */

export type IcuNode =
  | { type: 'text'; value: string }
  | { type: 'argument'; name: string }
  | { type: 'format'; name: string; format: 'number' | 'date' | 'time'; style: string | undefined }
  | { type: 'plural'; name: string; ordinal: boolean; offset: number; options: IcuOption[] }
  | { type: 'select'; name: string; options: IcuOption[] }
  | { type: 'pound' }
  | { type: 'tag'; name: string; children: IcuNode[] };

export interface IcuOption {
  /** `one`, `other`, `male`, or `=0` for an exact number. */
  selector: string;
  value: IcuNode[];
}

export class IcuSyntaxError extends Error {
  readonly kind: string;
  readonly offset: number;

  constructor(kind: string, offset: number) {
    super(`${kind} at offset ${offset}`);
    this.name = 'IcuSyntaxError';
    this.kind = kind;
    this.offset = offset;
  }
}

type ParentType = '' | 'plural' | 'selectordinal' | 'select';

const WHITE_SPACE = /\p{Pattern_White_Space}/u;
const IDENTIFIER = /[^\p{White_Space}\p{Pattern_Syntax}]*/uy;
const LETTER = /[a-zA-Z]/;
/** The characters a tag name may continue with: those of a custom element name. */
const TAG_NAME_RANGES: [number, number][] = [
  [0x2d, 0x2e],
  [0x30, 0x39],
  [0x5f, 0x5f],
  [0x61, 0x7a],
  [0x41, 0x5a],
  [0xb7, 0xb7],
  [0xc0, 0xd6],
  [0xd8, 0xf6],
  [0xf8, 0x37d],
  [0x37f, 0x1fff],
  [0x200c, 0x200d],
  [0x203f, 0x2040],
  [0x2070, 0x218f],
  [0x2c00, 0x2fef],
  [0x3001, 0xd7ff],
  [0xf900, 0xfdcf],
  [0xfdf0, 0xfffd],
];

function continuesTagName(character: string): boolean {
  const code = character.charCodeAt(0);
  return TAG_NAME_RANGES.some(([from, to]) => code >= from && code <= to);
}

export function parseIcu(message: string): IcuNode[] {
  let at = 0;

  const fail = (kind: string, offset = at): never => {
    throw new IcuSyntaxError(kind, offset);
  };
  const eof = (): boolean => at >= message.length;
  const char = (): string => message.charAt(at);
  const peek = (): string => message.charAt(at + 1);
  const skipSpace = (): void => {
    while (!eof() && WHITE_SPACE.test(char())) at += 1;
  };
  const eat = (text: string): boolean => {
    if (!message.startsWith(text, at)) return false;
    at += text.length;
    return true;
  };
  const identifier = (): string => {
    IDENTIFIER.lastIndex = at;
    const found = IDENTIFIER.exec(message)?.[0] ?? '';
    at += found.length;
    return found;
  };
  const isPlural = (parent: ParentType): boolean =>
    parent === 'plural' || parent === 'selectordinal';

  function parseMessage(depth: number, parent: ParentType, inTag: boolean): IcuNode[] {
    const nodes: IcuNode[] = [];
    const text = (value: string): void => {
      const last = nodes.at(-1);
      if (last?.type === 'text') last.value += value;
      else nodes.push({ type: 'text', value });
    };
    while (!eof()) {
      const current = char();
      if (current === '{') nodes.push(parseArgument(depth, inTag));
      else if (current === '}' && depth > 0) break;
      else if (current === '#' && isPlural(parent)) {
        at += 1;
        nodes.push({ type: 'pound' });
      } else if (current === '<' && peek() === '/') {
        if (inTag) break;
        fail('UNMATCHED_CLOSING_TAG');
      } else if (current === '<' && LETTER.test(peek())) {
        const tag = parseTag(depth, parent);
        if (typeof tag === 'string') text(tag);
        else nodes.push(tag);
      } else text(parseLiteral(depth, parent));
    }
    return nodes;
  }

  function parseLiteral(depth: number, parent: ParentType): string {
    let value = '';
    for (;;) {
      const piece = quoted(parent) ?? unquoted(depth, parent) ?? leftAngle();
      if (piece === undefined) break;
      value += piece;
    }
    return value;
  }

  function quoted(parent: ParentType): string | undefined {
    if (eof() || char() !== "'") return undefined;
    const next = peek();
    if (next === "'") {
      at += 2;
      return "'";
    }
    const quotes = ['{', '}', '<', '>'].includes(next) || (next === '#' && isPlural(parent));
    if (!quotes) return undefined;
    at += 1;
    let value = char();
    at += 1;
    while (!eof()) {
      if (char() === "'") {
        if (peek() === "'") {
          value += "'";
          at += 1;
        } else {
          at += 1;
          break;
        }
      } else value += char();
      at += 1;
    }
    return value;
  }

  function unquoted(depth: number, parent: ParentType): string | undefined {
    if (eof()) return undefined;
    const current = char();
    if (
      current === '<' ||
      current === '{' ||
      (current === '#' && isPlural(parent)) ||
      (current === '}' && depth > 0)
    )
      return undefined;
    // An apostrophe gets here only when quoted() found that it quotes nothing.
    at += 1;
    return current;
  }

  function leftAngle(): string | undefined {
    if (eof() || char() !== '<') return undefined;
    if (LETTER.test(peek()) || peek() === '/') return undefined;
    at += 1;
    return '<';
  }

  function parseTag(depth: number, parent: ParentType): IcuNode | string {
    const start = at;
    at += 1;
    const nameStart = at;
    at += 1;
    while (!eof() && continuesTagName(char())) at += 1;
    const name = message.slice(nameStart, at);
    skipSpace();
    if (eat('/>')) return `<${name}/>`;
    if (!eat('>')) fail('INVALID_TAG', start);
    const children = parseMessage(depth + 1, parent, true);
    const closeStart = at;
    if (!eat('</')) fail('UNCLOSED_TAG', start);
    if (eof() || !LETTER.test(char())) fail('INVALID_TAG', closeStart);
    const closeNameStart = at;
    at += 1;
    while (!eof() && continuesTagName(char())) at += 1;
    if (message.slice(closeNameStart, at) !== name) fail('UNMATCHED_CLOSING_TAG', closeNameStart);
    skipSpace();
    if (!eat('>')) fail('INVALID_TAG', closeStart);
    return { type: 'tag', name, children };
  }

  function closeArgument(open: number): void {
    if (eof() || char() !== '}') fail('EXPECT_ARGUMENT_CLOSING_BRACE', open);
    at += 1;
  }

  function parseArgument(depth: number, inTag: boolean): IcuNode {
    const open = at;
    at += 1;
    skipSpace();
    if (eof()) fail('EXPECT_ARGUMENT_CLOSING_BRACE', open);
    if (char() === '}') fail('EMPTY_ARGUMENT', open);
    const name = identifier();
    if (name === '') fail('MALFORMED_ARGUMENT', open);
    skipSpace();
    if (eof()) fail('EXPECT_ARGUMENT_CLOSING_BRACE', open);
    if (char() === '}') {
      at += 1;
      return { type: 'argument', name };
    }
    if (char() !== ',') fail('MALFORMED_ARGUMENT', open);
    at += 1;
    skipSpace();
    if (eof()) fail('EXPECT_ARGUMENT_CLOSING_BRACE', open);

    const typeStart = at;
    const type = identifier();
    switch (type) {
      case '':
        return fail('EXPECT_ARGUMENT_TYPE', typeStart);
      case 'number':
      case 'date':
      case 'time': {
        skipSpace();
        let style: string | undefined;
        if (eat(',')) {
          skipSpace();
          style = parseStyle().trimEnd();
          if (style === '') fail('EXPECT_ARGUMENT_STYLE');
        }
        closeArgument(open);
        return { type: 'format', name, format: type, style };
      }
      case 'plural':
      case 'selectordinal':
      case 'select': {
        skipSpace();
        if (!eat(',')) fail('EXPECT_SELECT_ARGUMENT_OPTIONS');
        skipSpace();
        let first = identifier();
        let offset = 0;
        if (type !== 'select' && first === 'offset') {
          if (!eat(':')) fail('EXPECT_PLURAL_ARGUMENT_OFFSET_VALUE');
          skipSpace();
          offset = integer('EXPECT_PLURAL_ARGUMENT_OFFSET_VALUE');
          skipSpace();
          first = identifier();
        }
        const options = parseOptions(depth, type, inTag, first);
        closeArgument(open);
        return type === 'select'
          ? { type: 'select', name, options }
          : { type: 'plural', name, ordinal: type === 'selectordinal', offset, options };
      }
      default:
        return fail('INVALID_ARGUMENT_TYPE', typeStart);
    }
  }

  function parseStyle(): string {
    const start = at;
    let nested = 0;
    while (!eof()) {
      const current = char();
      if (current === "'") {
        const quote = at;
        at += 1;
        const end = message.indexOf("'", at);
        if (end === -1) fail('UNCLOSED_QUOTE_IN_ARGUMENT_STYLE', quote);
        at = end + 1;
      } else if (current === '{') {
        nested += 1;
        at += 1;
      } else if (current === '}') {
        if (nested === 0) break;
        nested -= 1;
        at += 1;
      } else at += 1;
    }
    return message.slice(start, at);
  }

  function integer(missing: string): number {
    const start = at;
    if (!eat('+')) eat('-');
    const digits = at;
    while (!eof() && /[0-9]/.test(char())) at += 1;
    if (at === digits) fail(missing, start);
    return Number(message.slice(start, at));
  }

  function parseOptions(
    depth: number,
    type: 'plural' | 'selectordinal' | 'select',
    inTag: boolean,
    first: string,
  ): IcuOption[] {
    const options: IcuOption[] = [];
    const seen = new Set<string>();
    let selector = first;
    for (;;) {
      if (selector === '') {
        const start = at;
        if (type === 'select' || !eat('=')) break;
        integer('EXPECT_PLURAL_ARGUMENT_SELECTOR');
        selector = message.slice(start, at);
      }
      if (seen.has(selector)) fail('DUPLICATE_ARGUMENT_SELECTOR');
      seen.add(selector);
      skipSpace();
      const open = at;
      if (!eat('{')) fail('EXPECT_ARGUMENT_SELECTOR_FRAGMENT');
      const value = parseMessage(depth + 1, type, inTag);
      closeArgument(open);
      options.push({ selector, value });
      skipSpace();
      selector = identifier();
    }
    if (options.length === 0) fail('EXPECT_ARGUMENT_SELECTOR');
    if (!seen.has('other')) fail('MISSING_OTHER_CLAUSE');
    return options;
  }

  return parseMessage(0, '', false);
}
