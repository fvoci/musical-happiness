/**
 * What the conversion changes in the texts of the built-in templates.
 *
 * Two things, by decision: links that lead outside the app go, together with
 * the sentence that explains them (they are not rewritten), and the names of
 * the original product become `fvoci`. Every change is recorded, with the
 * text before and after, for the person who reviews the wording later.
 *
 * Nothing here reads the archive or decides where a text sits; build.ts
 * walks the templates and calls these.
 */
import type { DocNode } from '../../src/lib/editor/schema.ts';

export const NEW_NAME = 'fvoci';

/** The two names of the original, put together from halves (see tools/names). */
const OLD_PRODUCT = ['focal', 'board'].join('');
const OLD_COMPANY = ['matter', 'most'].join('');
const OLD_NAMES = new RegExp(`${OLD_PRODUCT}|${OLD_COMPANY}`, 'gi');

/**
 * The original also went by a plain word, written with a capital, where it
 * was one product of several: "switch over to Boards". After the company's
 * name it is one two-word name.
 */
const OLD_NAME_WITH_WORD = new RegExp(`(?:${OLD_PRODUCT}|${OLD_COMPANY})\\s+Boards\\b`, 'gi');
const PRODUCT_WORD = /\bBoards\b/g;

export type ChangeRule =
  /** One of the two names of the original, replaced. */
  | 'old-name'
  /** The plain word the original went by ("Boards"), replaced. A judgment, listed apart. */
  | 'product-word'
  /** A sentence that explained a link leading outside the app, removed with the link. */
  | 'outside-link'
  /** The value of a link property that led outside the app, removed. */
  | 'outside-link-value';

export interface Change {
  /** The key of the template. */
  template: string;
  /** What the text is part of: `board`, `card`, `view`, `property` or `option`, and which. */
  part: string;
  /** The title of that part (after the changes), for finding it on screen. */
  title: string;
  /** Which text of the part: `title`, `description`, `body`, `value of <property>`. */
  field: string;
  rule: ChangeRule;
  before: string;
  after: string;
}

/**
 * A text with the old names made unreadable, for the list of changes: the
 * list lives in a place where the names themselves must not appear.
 */
export function maskOldNames(text: string): string {
  return text.replace(OLD_NAMES, (name) =>
    name.toLowerCase() === OLD_PRODUCT ? '{old product name}' : '{old company name}',
  );
}

export const hasOldName = (text: string): boolean => {
  OLD_NAMES.lastIndex = 0;
  return OLD_NAMES.test(text);
};

export interface Renamed {
  text: string;
  /** Which rules changed the text; empty when it is as it was. */
  rules: ChangeRule[];
  /** How often the plain word was replaced. */
  productWords: number;
}

/** A text with the names of the original replaced. */
export function rename(text: string): Renamed {
  let productWords = 0;
  const named = text.replace(OLD_NAME_WITH_WORD, NEW_NAME).replace(OLD_NAMES, NEW_NAME);
  const worded = named.replace(PRODUCT_WORD, () => {
    productWords += 1;
    return NEW_NAME;
  });
  const rules: ChangeRule[] = [];
  if (named !== text) rules.push('old-name');
  if (worded !== named) rules.push('product-word');
  return { text: worded, rules, productWords };
}

/** Whether an address leads outside the app: it names a scheme, or a host. */
export function leadsOutside(href: string): boolean {
  // Browsers drop these characters before they read the scheme.
  // eslint-disable-next-line no-control-regex
  const address = href.replace(/[\u0000- \u007f]/g, '');
  return /^[a-z][a-z0-9+.-]*:/i.test(address) || address.startsWith('//');
}

/**
 * Whether the value of a link property leads outside the app. Such a value
 * is opened as an address whatever it looks like (`example.com` gets a
 * scheme put in front), so everything but a path inside the app does.
 */
export function valueLeadsOutside(value: string): boolean {
  const address = value.trim();
  return address !== '' && !/^\/(?!\/)/.test(address);
}

/** The ranges of the sentences of a text; together they cover all of it. */
export function sentenceRanges(text: string): [start: number, end: number][] {
  const ranges: [number, number][] = [];
  let start = 0;
  for (const match of text.matchAll(/[.!?…]+["'”’)\]]*\s+|\n+/g)) {
    const end = match.index + match[0].length;
    ranges.push([start, end]);
    start = end;
  }
  if (start < text.length) ranges.push([start, text.length]);
  return ranges;
}

const INLINE_PARENTS = new Set(['paragraph', 'heading']);

/** One character stands for a node that is not text, so that positions add up. */
const flatText = (node: DocNode): string =>
  node.type === 'text' ? (node.text ?? '') : node.type === 'hardBreak' ? '\n' : '￼';

const hrefOf = (mark: { attrs?: Record<string, unknown> }): string =>
  typeof mark.attrs?.href === 'string' ? mark.attrs.href : '';

const hasOutsideLink = (node: DocNode): boolean =>
  (node.marks ?? []).some((mark) => mark.type === 'link' && leadsOutside(hrefOf(mark)));

/** The text of a block as a reader sees it, a link written as `[text](address)`. */
function shownText(inline: readonly DocNode[]): string {
  return inline
    .map((node) => {
      const text = node.type === 'text' ? (node.text ?? '') : node.type === 'hardBreak' ? '\n' : '';
      const link = (node.marks ?? []).find((mark) => mark.type === 'link');
      return link ? `[${text}](${hrefOf(link)})` : text;
    })
    .join('');
}

/**
 * The inline content of one block without the sentences that hold a link
 * leading outside; undefined when nothing is left of it.
 */
function withoutLinkedSentences(inline: readonly DocNode[]): DocNode[] | undefined {
  const text = inline.map(flatText).join('');
  const linked: [number, number][] = [];
  let position = 0;
  for (const node of inline) {
    const length = flatText(node).length;
    if (hasOutsideLink(node)) linked.push([position, position + length]);
    position += length;
  }
  const kept = sentenceRanges(text).filter(
    ([start, end]) => !linked.some(([from, to]) => from < end && start < to),
  );

  const content: DocNode[] = [];
  position = 0;
  for (const node of inline) {
    const length = flatText(node).length;
    const [nodeStart, nodeEnd] = [position, position + length];
    position = nodeEnd;
    for (const [start, end] of kept) {
      const from = Math.max(start, nodeStart);
      const to = Math.min(end, nodeEnd);
      if (from >= to) continue;
      if (node.type !== 'text') {
        content.push(node);
        continue;
      }
      const slice = (node.text ?? '').slice(from - nodeStart, to - nodeStart);
      const last = content.at(-1);
      // Two kept sentences of one text node are one text node again.
      if (last?.type === 'text' && JSON.stringify(last.marks) === JSON.stringify(node.marks)) {
        last.text = (last.text ?? '') + slice;
      } else {
        content.push({ ...node, text: slice });
      }
    }
  }

  // The space that separated the removed sentence from its neighbour.
  const last = content.at(-1);
  if (last?.type === 'text') last.text = (last.text ?? '').trimEnd();
  const first = content[0];
  if (first?.type === 'text') first.text = (first.text ?? '').trimStart();
  const filled = content.filter((node) => node.type !== 'text' || node.text !== '');
  return filled.length > 0 ? filled : undefined;
}

export interface RemovedSentence {
  before: string;
  after: string;
}

function prune(node: DocNode, removed: RemovedSentence[]): DocNode | undefined {
  if (INLINE_PARENTS.has(node.type)) {
    const inline = node.content ?? [];
    if (!inline.some(hasOutsideLink)) return node;
    const content = withoutLinkedSentences(inline);
    removed.push({ before: shownText(inline), after: content ? shownText(content) : '' });
    return content ? { ...node, content } : undefined;
  }
  if (!node.content) return node;

  const content = node.content
    .map((child) => prune(child, removed))
    .filter((child): child is DocNode => child !== undefined);
  if (content.length === node.content.length && content.every((c, i) => c === node.content?.[i])) {
    return node;
  }
  // A container that lost everything goes too; the document itself is the caller's to judge.
  if (content.length === 0) return node.type === 'doc' ? { type: 'doc', content: [] } : undefined;
  // The first child of an item is a paragraph, also when its own text is what went.
  if ((node.type === 'listItem' || node.type === 'taskItem') && content[0]?.type !== 'paragraph') {
    content.unshift({ type: 'paragraph' });
  }
  return { ...node, content };
}

/**
 * A document without the sentences that explain a link leading outside the
 * app, and without what that leaves empty: paragraphs, list items, lists.
 * `doc` is undefined when nothing is left.
 */
export function removeOutsideLinks(doc: DocNode): {
  doc: DocNode | undefined;
  removed: RemovedSentence[];
} {
  const removed: RemovedSentence[] = [];
  const pruned = prune(doc, removed);
  return { doc: pruned?.content?.length ? pruned : undefined, removed };
}

/** A document with the names replaced in every text; the changed texts before and after. */
export function renameInDoc(doc: DocNode): {
  doc: DocNode;
  renamed: { before: string; after: string; rules: ChangeRule[] }[];
  productWords: number;
} {
  const renamed: { before: string; after: string; rules: ChangeRule[] }[] = [];
  let productWords = 0;
  const walk = (node: DocNode): DocNode => {
    if (INLINE_PARENTS.has(node.type) && node.content) {
      const before = shownText(node.content);
      const rules = new Set<ChangeRule>();
      const content = node.content.map((child) => {
        if (child.type !== 'text') return child;
        const result = rename(child.text ?? '');
        for (const rule of result.rules) rules.add(rule);
        productWords += result.productWords;
        return result.rules.length > 0 ? { ...child, text: result.text } : child;
      });
      if (rules.size === 0) return node;
      renamed.push({ before, after: shownText(content), rules: [...rules] });
      return { ...node, content };
    }
    return node.content ? { ...node, content: node.content.map(walk) } : node;
  };
  return { doc: walk(doc), renamed, productWords };
}
