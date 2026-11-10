/**
 * Turns a parsed ICU message into the inlang message format, the one
 * `@inlang/plugin-message-format` reads.
 *
 * What comes out behaves like the original for every input, or nothing comes
 * out: `NotConvertible` is thrown with the reason, and the caller leaves the
 * message out instead of approximating it.
 *
 * | ICU                                 | inlang                                                |
 * | ----------------------------------- | ----------------------------------------------------- |
 * | text, `''`, `'{'`                   | text, with `\{`, `\}` and `\\` escaped                |
 * | `{name}`                            | `{name}`                                              |
 * | `<b>…</b>`                          | `{#b}…{/b}`                                           |
 * | `{n, number}`, `integer`, `percent` | `{n: number}`, `style=integer`, `style=percent`       |
 * | `{n, plural, one {…} other {…}}`    | variants on `local nPlural = n: plural`               |
 * | `=0 {…}` in a plural                | a second selector, `local nPluralExact = n`           |
 * | `offset:1` and `#` in a plural      | `offset=1` on the plural, `{n: icu:pound offset=1}`   |
 * | `{n, selectordinal, …}`             | variants on `local nOrdinal = n: plural type=ordinal` |
 * | `{g, select, a {…} other {…}}`      | variants on the input `g`                             |
 *
 * `other` becomes the catch-all `*`, because that is what it means in ICU:
 * every value no other clause takes.
 *
 * Nested and neighbouring selections are multiplied out into variants. The
 * compiled message returns the first variant that matches, trying literal
 * keys before `*`, selector by selector. Whether that picks the variant ICU
 * would pick is not argued but checked: every combination of the keys is
 * tried (`verify`), and a message that fails is not converted.
 */
import type { IcuNode, IcuOption } from './icu.ts';

export interface ComplexMessage {
  declarations: string[];
  selectors: string[];
  match: Record<string, string>;
}

/** A value of a message file: a pattern, or one complex message in an array. */
export type InlangMessage = string | [ComplexMessage];

export class NotConvertible extends Error {
  constructor(reason: string) {
    super(reason);
    this.name = 'NotConvertible';
  }
}

/** Names that are safe both in a pattern and as a property of the inputs. */
const ARGUMENT_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
const TAG_NAME = /^[A-Za-z][A-Za-z0-9]*$/;
/** A match is written `selector=key, selector=key`. */
const MATCH_KEY = /^[^\s,=*]+$/u;
const NUMBER_STYLES = new Set(['integer', 'percent']);
/** The selections of one message may not multiply out to more than this. */
const MAX_COMBINATIONS = 4096;
/** A key no clause names: what `other` is tried with. */
const UNNAMED = '\u0000';

/** What a selector has to be for a variant to be the one ICU picks. */
type Condition = { is: string } | { not: ReadonlySet<string> };

interface Variant {
  conditions: Map<string, Condition>;
  pattern: string;
  /** The arguments that selected on the way to this variant. */
  selected: Set<string>;
}

interface State {
  inputs: string[];
  /** In the order the compiled message prefers them. */
  selectors: string[];
  /** Local variable name → its declaration. */
  locals: Map<string, string>;
}

interface Pound {
  name: string;
  offset: number;
}

export function escapeText(text: string): string {
  return text.replace(/[\\{}]/g, (character) => `\\${character}`);
}

function argument(name: string, state: State): string {
  if (!ARGUMENT_NAME.test(name)) {
    throw new NotConvertible(`the argument name "${name}" is not a plain identifier`);
  }
  if (!state.inputs.includes(name)) state.inputs.push(name);
  return name;
}

function declare(state: State, selector: string, declaration?: string): void {
  if (!state.selectors.includes(selector)) state.selectors.push(selector);
  if (declaration === undefined) return;
  const earlier = state.locals.get(selector);
  if (earlier !== undefined && earlier !== declaration) {
    throw new NotConvertible(`"${selector}" would be declared in two ways`);
  }
  state.locals.set(selector, declaration);
}

function cross(left: Variant[], right: Variant[]): Variant[] {
  if (left.length * right.length > MAX_COMBINATIONS) {
    throw new NotConvertible(`more than ${MAX_COMBINATIONS} variants`);
  }
  return left.flatMap((a) =>
    right.map((b) => {
      const twice = [...b.selected].find((name) => a.selected.has(name));
      if (twice !== undefined) {
        throw new NotConvertible(`the argument "${twice}" selects more than once`);
      }
      return {
        conditions: new Map([...a.conditions, ...b.conditions]),
        pattern: a.pattern + b.pattern,
        selected: new Set([...a.selected, ...b.selected]),
      };
    }),
  );
}

/** The variants of one plural or select: each clause, under the conditions that lead to it. */
function clauses(
  name: string,
  options: IcuOption[],
  conditionsOf: (selector: string) => [string, Condition][],
  state: State,
  pound: Pound | undefined,
): Variant[] {
  return options.flatMap((option) => {
    const own: Variant = {
      conditions: new Map(
        conditionsOf(option.selector).filter(([, c]) => 'is' in c || c.not.size > 0),
      ),
      pattern: '',
      selected: new Set([name]),
    };
    return cross([own], expand(option.value, state, pound));
  });
}

function expand(nodes: IcuNode[], state: State, pound: Pound | undefined): Variant[] {
  let variants: Variant[] = [{ conditions: new Map(), pattern: '', selected: new Set() }];
  const append = (pattern: string): void => {
    for (const variant of variants) variant.pattern += pattern;
  };

  for (const node of nodes) {
    switch (node.type) {
      case 'text':
        append(escapeText(node.value));
        break;
      case 'argument':
        append(`{${argument(node.name, state)}}`);
        break;
      case 'format': {
        if (node.format !== 'number') {
          throw new NotConvertible(
            `the ${node.format} format of "${node.name}": its styles are lists of fields in the original and date styles here`,
          );
        }
        if (node.style !== undefined && !NUMBER_STYLES.has(node.style)) {
          throw new NotConvertible(`the number style "${node.style}" of "${node.name}"`);
        }
        const style = node.style === undefined ? '' : ` style=${node.style}`;
        append(`{${argument(node.name, state)}: number${style}}`);
        break;
      }
      case 'pound': {
        // The parser yields this node only directly inside a plural.
        if (!pound) throw new Error('a "#" outside of a plural');
        append(`{${pound.name}: icu:pound${pound.offset === 0 ? '' : ` offset=${pound.offset}`}}`);
        break;
      }
      case 'tag': {
        if (!TAG_NAME.test(node.name)) {
          throw new NotConvertible(`the tag name "${node.name}" is not letters and digits`);
        }
        append(`{#${node.name}}`);
        variants = cross(variants, expand(node.children, state, pound));
        append(`{/${node.name}}`);
        break;
      }
      case 'plural': {
        const name = argument(node.name, state);
        const category = `${name}${node.ordinal ? 'Ordinal' : 'Plural'}`;
        const exact = `${category}Exact`;
        const options =
          (node.ordinal ? ' type=ordinal' : '') +
          (node.offset === 0 ? '' : ` offset=${node.offset}`);
        declare(state, exact, `local ${exact} = ${name}`);
        declare(state, category, `local ${category} = ${name}: plural${options}`);

        const number = (selector: string): string => String(Number(selector.slice(1)));
        const exacts = new Set<string>();
        const categories = new Set<string>();
        for (const { selector } of node.options) {
          if (selector.startsWith('=')) exacts.add(number(selector));
          else if (selector !== 'other') categories.add(selector);
        }
        const inner = clauses(
          name,
          node.options,
          (selector) => {
            if (selector.startsWith('=')) return [[exact, { is: number(selector) }]];
            return [
              [exact, { not: exacts }],
              [category, selector === 'other' ? { not: categories } : { is: selector }],
            ];
          },
          state,
          { name, offset: node.offset },
        );
        variants = cross(variants, inner);
        break;
      }
      case 'select': {
        const name = argument(node.name, state);
        declare(state, name);
        const keys = new Set<string>();
        for (const { selector } of node.options) {
          if (selector === 'other') continue;
          if (!MATCH_KEY.test(selector)) {
            throw new NotConvertible(`the select key "${selector}" of "${name}"`);
          }
          keys.add(selector);
        }
        const inner = clauses(
          name,
          node.options,
          (selector) => [[name, selector === 'other' ? { not: keys } : { is: selector }]],
          state,
          // A "#" inside a select is text, even when a plural is around it.
          undefined,
        );
        variants = cross(variants, inner);
        break;
      }
    }
  }
  return variants;
}

const literal = (variant: Variant, selector: string): string | undefined => {
  const condition = variant.conditions.get(selector);
  return condition && 'is' in condition ? condition.is : undefined;
};

/** Literal keys before the catch-all, selector by selector: the order the compiled message tries. */
function byPreference(selectors: string[]): (a: Variant, b: Variant) => number {
  return (a, b) => {
    for (const selector of selectors) {
      const difference =
        Number(literal(a, selector) === undefined) - Number(literal(b, selector) === undefined);
      if (difference !== 0) return difference;
    }
    return 0;
  };
}

/**
 * Tries every combination of keys: the variant the compiled message returns
 * (the first, in order, whose literal keys all match) has to be the one whose
 * conditions hold, which is the one ICU picks.
 */
function verify(ordered: Variant[], selectors: string[]): void {
  const domains = selectors.map((selector) => [
    ...new Set(ordered.flatMap((variant) => literal(variant, selector) ?? [])),
    UNNAMED,
  ]);
  const combinations = domains.reduce((product, domain) => product * domain.length, 1);
  if (combinations > MAX_COMBINATIONS) {
    throw new NotConvertible(`more than ${MAX_COMBINATIONS} combinations of keys`);
  }
  for (let index = 0; index < combinations; index += 1) {
    const value = new Map<string, string>();
    let rest = index;
    selectors.forEach((selector, at) => {
      const domain = domains[at] ?? [];
      value.set(selector, domain[rest % domain.length] ?? UNNAMED);
      rest = Math.floor(rest / domain.length);
    });
    const holds = (variant: Variant): boolean =>
      [...variant.conditions].every(([selector, condition]) => {
        const key = value.get(selector) ?? UNNAMED;
        return 'is' in condition ? condition.is === key : !condition.not.has(key);
      });
    const matches = (variant: Variant): boolean =>
      selectors.every((selector) => {
        const key = literal(variant, selector);
        return key === undefined || key === value.get(selector);
      });
    const right = ordered.filter(holds);
    if (right.length !== 1) throw new Error('exactly one variant has to hold for a combination');
    if (ordered.find(matches) !== right[0]) {
      throw new NotConvertible('the first matching variant would not be the one ICU picks');
    }
  }
}

export function toInlang(nodes: IcuNode[]): InlangMessage {
  const state: State = { inputs: [], selectors: [], locals: new Map() };
  const variants = expand(nodes, state, undefined);

  // A selector that no variant names with a literal key selects nothing.
  const selectors = state.selectors.filter((selector) =>
    variants.some((variant) => literal(variant, selector) !== undefined),
  );
  const [only] = variants;
  if (selectors.length === 0) {
    if (variants.length !== 1 || !only) throw new Error('one variant expected without a selector');
    return only.pattern;
  }
  const taken = [...state.locals.keys()].find((local) => state.inputs.includes(local));
  if (taken !== undefined) {
    throw new NotConvertible(`the argument "${taken}" has the name of a local variable`);
  }

  const ordered = variants.toSorted(byPreference(selectors));
  verify(ordered, selectors);

  const match: Record<string, string> = {};
  for (const variant of ordered) {
    const key = selectors
      .map((selector) => `${selector}=${literal(variant, selector) ?? '*'}`)
      .join(', ');
    if (key in match) throw new NotConvertible(`two variants for "${key}"`);
    match[key] = variant.pattern;
  }
  return [
    {
      declarations: [
        ...state.inputs.map((name) => `input ${name}`),
        ...selectors.flatMap((selector) => state.locals.get(selector) ?? []),
      ],
      selectors,
      match,
    },
  ];
}

/** What a message asks of the code that calls it, and what it is made of. */
export interface Shape {
  arguments: Set<string>;
  tags: Set<string>;
  /** How often each kind of construct occurs. */
  constructs: Map<string, number>;
}

export function shapeOf(nodes: IcuNode[], shape?: Shape): Shape {
  const result: Shape = shape ?? { arguments: new Set(), tags: new Set(), constructs: new Map() };
  const count = (kind: string): void => {
    result.constructs.set(kind, (result.constructs.get(kind) ?? 0) + 1);
  };
  for (const node of nodes) {
    switch (node.type) {
      case 'text':
        break;
      case 'pound':
        count('#');
        break;
      case 'argument':
        result.arguments.add(node.name);
        count('argument');
        break;
      case 'format':
        result.arguments.add(node.name);
        count(node.format);
        break;
      case 'tag':
        result.tags.add(node.name);
        count('tag');
        shapeOf(node.children, result);
        break;
      case 'plural':
      case 'select':
        result.arguments.add(node.name);
        count(node.type === 'select' ? 'select' : node.ordinal ? 'selectordinal' : 'plural');
        for (const option of node.options) shapeOf(option.value, result);
        break;
    }
  }
  return result;
}
