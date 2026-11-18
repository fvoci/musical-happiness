/**
 * The HTML half of the DOM docx-preview draws into in the Word worker
 * (`docx-worker.ts`), where browsers have no `document`: elements, text,
 * comments and fragments with the members docx-preview's renderer uses
 * (`createElement(NS)`, `appendChild`, `setAttribute`, `style`, `classList`,
 * and the properties `id`, `href`, `src`, `colSpan`, `rowSpan`), and a
 * serializer that turns what was drawn into HTML for the frame.
 *
 * Every node made counts toward a ceiling; past it every further one throws
 * `TooMuch`, and the ceiling stays passed, so that a renderer which catches
 * an error somewhere still ends with the document refused. The HTML is held
 * to a length; past it the serializer throws `TooMuch` too.
 *
 * What is written is only what the renderer made: tag and attribute names
 * that are not plain names, and attributes that would run script (`on…`),
 * are left out, and text, attribute values and style sheets are escaped.
 */
import { TooMuch } from './worker-xml.ts';

/** The most nodes a document may draw: many times a long report's. */
export const HTML_NODE_LIMIT = 2_000_000;
/** The longest HTML handed to the frame, in characters. */
export const HTML_LENGTH_LIMIT = 24_000_000;

const HTML_NS = 'http://www.w3.org/1999/xhtml';
const TAG = /^[A-Za-z][A-Za-z0-9-]*$/;
const ATTRIBUTE = /^[A-Za-z_:][A-Za-z0-9_.:-]*$/;
const VOID = new Set([
  'area',
  'br',
  'col',
  'embed',
  'hr',
  'img',
  'input',
  'link',
  'meta',
  'source',
  'track',
  'wbr',
]);

/** Counts the nodes made against `HTML_NODE_LIMIT`; once past it, stays past. */
export class Budget {
  #made = 0;

  get passed(): boolean {
    return this.#made > HTML_NODE_LIMIT;
  }

  add(): void {
    this.#made += 1;
    if (this.passed) throw new TooMuch('elements');
  }
}

/** The base of every node: what `instanceof Node` and `Node.ELEMENT_NODE` find. */
export class Node {
  static readonly ELEMENT_NODE = 1;
  static readonly TEXT_NODE = 3;
  static readonly COMMENT_NODE = 8;
  static readonly DOCUMENT_FRAGMENT_NODE = 11;
  parentNode: Parent | null = null;
}

export class Text extends Node {
  readonly nodeType = 3;
  readonly nodeName = '#text';
  data: string;

  constructor(data: string) {
    super();
    this.data = data;
  }

  get textContent(): string {
    return this.data;
  }
}

export class Comment extends Node {
  readonly nodeType = 8;
  readonly nodeName = '#comment';
}

export class Parent extends Node {
  readonly childNodes: Node[] = [];

  appendChild<T extends Node>(child: T): T {
    if (child instanceof Fragment) {
      for (const each of child.childNodes.splice(0)) this.appendChild(each);
      return child;
    }
    child.parentNode?.removeChild(child);
    child.parentNode = this;
    this.childNodes.push(child);
    return child;
  }

  append(...children: (Node | string)[]): void {
    for (const child of children)
      this.appendChild(typeof child === 'string' ? new Text(child) : child);
  }

  removeChild<T extends Node>(child: T): T {
    const at = this.childNodes.indexOf(child);
    if (at >= 0) this.childNodes.splice(at, 1);
    child.parentNode = null;
    return child;
  }

  get firstChild(): Node | null {
    return this.childNodes[0] ?? null;
  }

  get firstElementChild(): Element | null {
    return this.childNodes.find((node) => node instanceof Element) ?? null;
  }

  get textContent(): string {
    return this.childNodes
      .map((node) => (node instanceof Text || node instanceof Parent ? node.textContent : ''))
      .join('');
  }

  set textContent(text: string) {
    for (const child of this.childNodes.splice(0)) child.parentNode = null;
    if (text !== '') this.appendChild(new Text(text));
  }

  /** Only what the renderer sets: nothing, or a no-break space. */
  set innerHTML(html: string) {
    this.textContent = html === '&nbsp;' ? ' ' : html;
  }
}

export class Fragment extends Parent {
  readonly nodeType = 11;
  readonly nodeName = '#document-fragment';
}

/** A style declaration: whatever is set on it, by camel-case or by CSS name. */
export type Style = Record<string, string | undefined>;

const cssName = (name: string): string =>
  name.startsWith('--') || name.includes('-')
    ? name
    : name.replace(/[A-Z]/g, (upper) => `-${upper.toLowerCase()}`);

export class Element extends Parent {
  readonly nodeType = 1;
  readonly tagName: string;
  readonly namespaceURI: string;
  readonly style: Style = {};
  readonly attributes = new Map<string, string>();
  readonly classList = {
    add: (...names: string[]): void => {
      const now = (this.attributes.get('class') ?? '').split(' ').filter(Boolean);
      this.attributes.set('class', [...new Set([...now, ...names])].join(' '));
    },
  };

  constructor(tagName: string, namespaceURI: string) {
    super();
    this.tagName = tagName;
    this.namespaceURI = namespaceURI;
  }

  get nodeName(): string {
    return this.namespaceURI === HTML_NS ? this.tagName.toUpperCase() : this.tagName;
  }

  setAttribute(name: string, value: unknown): void {
    this.attributes.set(name, String(value));
  }

  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }

  #reflect(name: string): { get: () => string; set: (value: unknown) => void } {
    return {
      get: () => this.attributes.get(name) ?? '',
      set: (value) => {
        this.setAttribute(name, value);
      },
    };
  }

  get className(): string {
    return this.#reflect('class').get();
  }
  set className(value: string) {
    this.#reflect('class').set(value);
  }
  get id(): string {
    return this.#reflect('id').get();
  }
  set id(value: string) {
    this.#reflect('id').set(value);
  }
  get href(): string {
    return this.#reflect('href').get();
  }
  set href(value: string) {
    this.#reflect('href').set(value);
  }
  get src(): string {
    return this.#reflect('src').get();
  }
  set src(value: string) {
    this.#reflect('src').set(value);
  }
  get colSpan(): number {
    return Number(this.#reflect('colspan').get() || 1);
  }
  set colSpan(value: number) {
    this.#reflect('colspan').set(value);
  }
  get rowSpan(): number {
    return Number(this.#reflect('rowspan').get() || 1);
  }
  set rowSpan(value: number) {
    this.#reflect('rowspan').set(value);
  }
}

/** The `document` docx-preview draws with: every node it makes is counted. */
export function documentFor(budget: Budget): Record<string, unknown> {
  const counted = <T>(make: () => T): T => {
    budget.add();
    return make();
  };
  return {
    createElement: (tag: string) => counted(() => new Element(tag.toLowerCase(), HTML_NS)),
    createElementNS: (namespace: string | null, tag: string) =>
      counted(
        () => new Element(namespace === HTML_NS ? tag.toLowerCase() : tag, namespace ?? HTML_NS),
      ),
    createTextNode: (text: string) => counted(() => new Text(text)),
    createComment: () => counted(() => new Comment()),
    createDocumentFragment: () => counted(() => new Fragment()),
    createRange: () => {
      throw new Error('no layout in the worker');
    },
  };
}

const escapeText = (text: string): string =>
  text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const escapeValue = (text: string): string => escapeText(text).replace(/"/g, '&quot;');

function opening(element: Element): string {
  const attributes = new Map(element.attributes);
  const style = Object.entries(element.style)
    .filter((entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1] !== '')
    .map(([name, value]) => `${cssName(name)}: ${value}`)
    .join('; ');
  if (style) attributes.set('style', [attributes.get('style'), style].filter(Boolean).join('; '));
  let text = `<${element.tagName}`;
  for (const [name, value] of attributes) {
    if (ATTRIBUTE.test(name) && !/^on/i.test(name)) text += ` ${name}="${escapeValue(value)}"`;
  }
  return `${text}>`;
}

/** The HTML of the nodes, in order; throws `TooMuch` once it would pass `limit` characters. */
export function serialize(nodes: readonly Node[], limit = HTML_LENGTH_LIMIT): string {
  const parts: string[] = [];
  let length = 0;
  const write = (text: string): void => {
    length += text.length;
    if (length > limit) throw new TooMuch('HTML');
    parts.push(text);
  };
  // Nodes to write, and the closing tags to write after an element's children.
  const stack: (Node | string)[] = nodes.toReversed();
  while (stack.length > 0) {
    const next = stack.pop();
    if (typeof next === 'string') write(next);
    else if (next instanceof Text) {
      const inStyle = next.parentNode instanceof Element && next.parentNode.tagName === 'style';
      // A style sheet is written as it is, but cannot close its element.
      write(inStyle ? next.data.replace(/<\/(style)/gi, '<\\/$1') : escapeText(next.data));
    } else if (next instanceof Element) {
      const named = TAG.test(next.tagName);
      if (named) write(opening(next));
      if (named && VOID.has(next.tagName)) continue;
      if (named) stack.push(`</${next.tagName}>`);
      // One at a time: a spread of a long list would overflow the call stack.
      for (const child of next.childNodes.toReversed()) stack.push(child);
    } else if (next instanceof Parent) {
      for (const child of next.childNodes.toReversed()) stack.push(child);
    }
  }
  return parts.join('');
}
