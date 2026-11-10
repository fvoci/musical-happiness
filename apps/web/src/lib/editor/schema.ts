/**
 * What a document body may contain.
 *
 * The body of a card or a comment is one document: a tree of nodes with
 * marks on its text, stored as JSON. The server accepts a body only when
 * every node, mark and attribute in it is on this list; so the editor must
 * not be able to produce anything else, and what it loads is checked the
 * same way. This file is that list as data, and a validator over it. The
 * editor's extensions (extensions.ts) are configured to match, and a test
 * holds the two together.
 */

/** The version of this list. A stored body carries the version it was written under. */
export const BODY_VERSION = 1;

export const MAX_DEPTH = 32;
export const MAX_NODES = 20_000;

export interface DocMark {
  type: string;
  attrs?: Record<string, unknown>;
}

export interface DocNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: DocNode[];
  marks?: DocMark[];
  text?: string;
}

/** Whether a value is acceptable for an attribute. */
type AttributeRule = (value: unknown) => boolean;

interface AttributeSpec {
  rule: AttributeRule;
  required: boolean;
}

interface NodeSpec {
  attrs: Record<string, AttributeSpec>;
  /** The node types allowed directly inside; none for a leaf. */
  children: readonly string[];
  /** Fewest children the node must have. */
  minChildren: number;
}

const required = (rule: AttributeRule): AttributeSpec => ({ rule, required: true });
const optional = (rule: AttributeRule): AttributeSpec => ({ rule, required: false });

const isText =
  (max: number): AttributeRule =>
  (value) =>
    typeof value === 'string' && value.length <= max;
const isNonEmptyText =
  (max: number): AttributeRule =>
  (value) =>
    typeof value === 'string' && value.length > 0 && value.length <= max;
const isIntegerBetween =
  (low: number, high: number): AttributeRule =>
  (value) =>
    typeof value === 'number' && Number.isInteger(value) && value >= low && value <= high;
const isBoolean: AttributeRule = (value) => typeof value === 'boolean';
/** A language tag of a code block: `rust`, `c++`, `objective-c`. */
const isLanguage: AttributeRule = (value) =>
  typeof value === 'string' && /^[A-Za-z0-9+#._-]{1,40}$/.test(value);

/**
 * The schemes a link may use. Anything without a scheme is a path inside
 * the app. Whitespace and control characters do not hide a scheme: browsers
 * drop them before reading one, so this does too. A leading `//` or a
 * backslash would leave the site and is refused.
 */
const LINK_SCHEMES = ['http', 'https', 'mailto'];

export function isAllowedHref(href: unknown): href is string {
  if (typeof href !== 'string' || href.length === 0 || href.length > 2048) return false;
  // eslint-disable-next-line no-control-regex -- control characters are the point
  const compact = href.replace(/[\u0000- \u007f]+/g, '');
  if (compact.length === 0) return false;
  const scheme = /^([A-Za-z][A-Za-z0-9+.-]*):/.exec(compact)?.[1];
  if (scheme !== undefined) return LINK_SCHEMES.includes(scheme.toLowerCase());
  return !compact.startsWith('//') && !compact.includes('\\');
}

const BLOCKS = [
  'paragraph',
  'heading',
  'bulletList',
  'orderedList',
  'taskList',
  'blockquote',
  'codeBlock',
  'horizontalRule',
  'image',
] as const;
const INLINE = ['text', 'hardBreak', 'mention'] as const;

export const NODE_SPECS: Record<string, NodeSpec> = {
  doc: { attrs: {}, children: BLOCKS, minChildren: 1 },
  paragraph: { attrs: {}, children: INLINE, minChildren: 0 },
  heading: { attrs: { level: required(isIntegerBetween(1, 3)) }, children: INLINE, minChildren: 0 },
  bulletList: { attrs: {}, children: ['listItem'], minChildren: 1 },
  orderedList: {
    attrs: { start: optional(isIntegerBetween(0, 1_000_000_000)) },
    children: ['listItem'],
    minChildren: 1,
  },
  listItem: { attrs: {}, children: BLOCKS, minChildren: 1 },
  taskList: { attrs: {}, children: ['taskItem'], minChildren: 1 },
  taskItem: { attrs: { checked: required(isBoolean) }, children: BLOCKS, minChildren: 1 },
  blockquote: { attrs: {}, children: BLOCKS, minChildren: 1 },
  codeBlock: { attrs: { language: optional(isLanguage) }, children: ['text'], minChildren: 0 },
  horizontalRule: { attrs: {}, children: [], minChildren: 0 },
  hardBreak: { attrs: {}, children: [], minChildren: 0 },
  image: {
    attrs: { fileId: required(isNonEmptyText(100)), alt: optional(isText(1000)) },
    children: [],
    minChildren: 0,
  },
  mention: { attrs: { userId: required(isNonEmptyText(100)) }, children: [], minChildren: 0 },
  text: { attrs: {}, children: [], minChildren: 0 },
};

export const MARK_SPECS: Record<string, Record<string, AttributeSpec>> = {
  bold: {},
  italic: {},
  strike: {},
  code: {},
  link: { href: required(isAllowedHref) },
};

/** Nodes whose first child has to be a paragraph (the editor's own rule for list entries). */
const STARTS_WITH_PARAGRAPH = new Set(['listItem', 'taskItem']);

function checkAttributes(
  where: string,
  attrs: Record<string, unknown> | undefined,
  specs: Record<string, AttributeSpec>,
  problems: string[],
): void {
  const given = attrs ?? {};
  for (const name of Object.keys(given)) {
    const spec = specs[name];
    if (!spec) problems.push(`${where}: unknown attribute "${name}"`);
    else if (!spec.rule(given[name]))
      problems.push(`${where}: attribute "${name}" has an invalid value`);
  }
  for (const [name, spec] of Object.entries(specs)) {
    if (spec.required && !(name in given))
      problems.push(`${where}: attribute "${name}" is missing`);
  }
}

/**
 * Everything that is wrong with a document; an empty list means the server
 * will take it. Checked are the kinds of nodes and marks, their attributes,
 * which node may be inside which, and the limits on depth and size.
 */
export function validateDoc(doc: unknown): string[] {
  const problems: string[] = [];
  let count = 0;

  function visit(
    node: unknown,
    where: string,
    depth: number,
    allowed: readonly string[] | undefined,
  ): void {
    if (problems.length >= 20) return;
    if (typeof node !== 'object' || node === null || Array.isArray(node)) {
      problems.push(`${where}: not a node`);
      return;
    }
    const { type, attrs, content, marks, text, ...rest } = node as DocNode &
      Record<string, unknown>;
    const typeName = typeof type === 'string' ? type : '(no type)';
    const at = `${where}/${typeName}`;
    count += 1;
    if (count > MAX_NODES) {
      problems.push(`more than ${MAX_NODES} nodes`);
      return;
    }
    if (depth > MAX_DEPTH) {
      problems.push(`${at}: deeper than ${MAX_DEPTH}`);
      return;
    }
    const spec = typeof type === 'string' ? NODE_SPECS[type] : undefined;
    if (!spec) {
      problems.push(`${where}: unknown node type "${typeName}"`);
      return;
    }
    if (allowed && !allowed.includes(type))
      problems.push(`${at}: not allowed inside ${where || 'the root'}`);
    for (const key of Object.keys(rest)) problems.push(`${at}: unknown member "${key}"`);
    checkAttributes(at, attrs, spec.attrs, problems);

    if (type === 'text') {
      if (typeof text !== 'string' || text.length === 0) problems.push(`${at}: text is empty`);
      for (const mark of marks ?? []) {
        const markSpec = MARK_SPECS[mark.type];
        if (!markSpec) problems.push(`${at}: unknown mark "${mark.type}"`);
        else checkAttributes(`${at}@${mark.type}`, mark.attrs, markSpec, problems);
      }
      if (new Set((marks ?? []).map((mark) => mark.type)).size !== (marks ?? []).length) {
        problems.push(`${at}: a mark is given twice`);
      }
    } else {
      if (text !== undefined) problems.push(`${at}: only text nodes have text`);
      if (marks !== undefined) problems.push(`${at}: only text nodes have marks`);
    }

    const children = content ?? [];
    if (!Array.isArray(children)) {
      problems.push(`${at}: content is not a list`);
      return;
    }
    if (children.length < spec.minChildren)
      problems.push(`${at}: needs at least ${spec.minChildren} child`);
    if (children.length > 0 && spec.children.length === 0)
      problems.push(`${at}: cannot have content`);
    if (STARTS_WITH_PARAGRAPH.has(type) && children[0]?.type !== 'paragraph') {
      problems.push(`${at}: has to start with a paragraph`);
    }
    if (type === 'codeBlock' && children.some((child) => child.marks !== undefined)) {
      problems.push(`${at}: text in a code block has no marks`);
    }
    children.forEach((child, index) => {
      visit(child, `${at}[${index}]`, depth + 1, spec.children);
    });
  }

  visit(doc, '', 1, ['doc']);
  return problems;
}

/** The document an empty body stands for. */
export function emptyDoc(): DocNode {
  return { type: 'doc', content: [{ type: 'paragraph' }] };
}

/**
 * A document in the form that is stored: attributes without a value are left
 * out, and so is an attribute list that became empty. The editor gives every
 * attribute it knows a slot, `null` when unset; the stored form has no nulls.
 */
export function toStoredDoc(node: DocNode): DocNode {
  const stored: DocNode = { type: node.type };
  const attrs = Object.entries(node.attrs ?? {}).filter(
    ([, value]) => value !== null && value !== undefined,
  );
  if (attrs.length > 0) stored.attrs = Object.fromEntries(attrs);
  if (node.content) stored.content = node.content.map(toStoredDoc);
  if (node.marks) {
    stored.marks = node.marks.map((mark) => {
      const markAttrs = Object.entries(mark.attrs ?? {}).filter(
        ([, value]) => value !== null && value !== undefined,
      );
      return markAttrs.length > 0
        ? { type: mark.type, attrs: Object.fromEntries(markAttrs) }
        : { type: mark.type };
    });
  }
  if (node.text !== undefined) stored.text = node.text;
  return stored;
}

/** The size of a document as it travels: the bytes of its JSON in UTF-8. */
export function storedSize(doc: DocNode): number {
  return new TextEncoder().encode(JSON.stringify(doc)).length;
}

/** How many tasks a document has and how many are ticked, for the badge on a card. */
export function countTasks(node: DocNode): { total: number; checked: number } {
  const count = { total: 0, checked: 0 };
  if (node.type === 'taskItem') {
    count.total += 1;
    if (node.attrs?.checked === true) count.checked += 1;
  }
  for (const child of node.content ?? []) {
    const inner = countTasks(child);
    count.total += inner.total;
    count.checked += inner.checked;
  }
  return count;
}

const TEXT_BLOCKS = new Set(['paragraph', 'heading', 'codeBlock']);

/**
 * The words of a document without its structure, one line per paragraph:
 * for search, previews and notifications. `mentionLabel` names a mentioned
 * user.
 */
export function plainText(
  node: DocNode,
  mentionLabel: (userId: string) => string = (id) => id,
): string {
  const lines: string[] = [];
  function visit(current: DocNode): string {
    if (current.type === 'text') return current.text ?? '';
    if (current.type === 'hardBreak') return '\n';
    if (current.type === 'mention') return `@${mentionLabel(String(current.attrs?.userId))}`;
    const inner = (current.content ?? []).map(visit).join('');
    if (TEXT_BLOCKS.has(current.type)) lines.push(inner);
    return '';
  }
  visit(node);
  return lines.filter((line) => line !== '').join('\n');
}
