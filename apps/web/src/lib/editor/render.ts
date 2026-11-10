/**
 * Draws a document body for reading, without the editor.
 *
 * A card preview, a list of comments and a shared board show bodies that
 * nobody is editing; the editor is far too heavy to load for that. This
 * turns a stored document straight into DOM nodes: the same elements,
 * classes and `data-` attributes the editor produces, so that editor.css
 * styles both and a body looks the same whether or not it is being edited.
 * A test draws real documents both ways and compares the two.
 *
 * What makes this safe to feed with whatever the server sends:
 *
 * - A document is drawn only when the validator (schema.ts) accepts it.
 *   Anything else gives a failure and no nodes at all.
 * - Text goes in as text nodes and attributes through `setAttribute`.
 *   Nothing here parses markup.
 * - Only the elements and attributes named in this file are ever created.
 *   A link gets its address only when `isAllowedHref` accepts it here, again,
 *   whatever the validator said.
 */
import { IMAGE_ATTRIBUTES, LINK_ATTRIBUTES } from './drawing.ts';
import { fileUrl } from './files.ts';
import { isAllowedHref, NODE_SPECS, validateDoc, type DocMark, type DocNode } from './schema.ts';

export interface RenderBodyOptions {
  /**
   * The name of a mentioned user. A document holds only the identifier; when
   * this gives no name the identifier is shown, as in the editor.
   */
  resolveUser?: ((userId: string) => string | undefined) | undefined;
  /**
   * The accessible name of the checkbox of a task, from the text of the task
   * (its first paragraph). Without it the name is that text.
   */
  taskLabel?: ((text: string, checked: boolean) => string) | undefined;
  /** The document the nodes are created in; the page's own unless given. */
  document?: Document | undefined;
}

export type RenderedBody =
  | {
      ok: true;
      /** The body has nothing in it: there is no document, or only an empty paragraph. */
      empty: boolean;
      /** The blocks of the body, to be placed inside an element of class `doc-body`. */
      fragment: DocumentFragment;
    }
  | {
      ok: false;
      /** Why the document is not drawn, in the validator's words. */
      problems: string[];
    };

/**
 * The marks in the order the editor nests them, outermost first. The stored
 * order of marks has no meaning; the editor sorts them like this.
 */
const MARK_ORDER = ['link', 'bold', 'italic', 'strike', 'code'];
const MARK_TAGS: Record<string, string> = {
  bold: 'strong',
  italic: 'em',
  strike: 's',
  code: 'code',
};
const HEADING_TAGS: Record<number, string> = { 1: 'h1', 2: 'h2', 3: 'h3' };
const INLINE = new Set(['text', 'hardBreak', 'mention']);

const TEXT_NODE = 3;

/** A mark as it is drawn: the element, and what tells it from its neighbour. */
interface DrawnMark {
  /** Two neighbouring texts share one element when this is equal. */
  key: string;
  tag: string;
  href?: string;
}

// The validator has been over the document before any of this runs. What
// follows still takes nothing on trust that would cost little to check: a
// value is used only when it has the type it should have.

const textAttribute = (node: DocNode, name: string): string | undefined => {
  const value = node.attrs?.[name];
  return typeof value === 'string' ? value : undefined;
};

/** The members of a list that are objects; none when it is not a list. */
function objectsIn<T extends object>(list: unknown): T[] {
  if (!Array.isArray(list)) return [];
  return (list as unknown[]).filter(
    (member): member is T => typeof member === 'object' && member !== null,
  );
}

const childrenOf = (node: DocNode): DocNode[] => objectsIn<DocNode>(node.content);

function headingTag(node: DocNode): string {
  const level = node.attrs?.level;
  return (typeof level === 'number' ? HEADING_TAGS[level] : undefined) ?? 'p';
}

/**
 * The marks that are drawn around a piece of inline content, outermost
 * first. The validator lets only text have marks; the editor also draws them
 * around a line break or a mention, and so would this.
 */
function drawnMarks(marks: unknown): DrawnMark[] {
  const drawn: (DrawnMark & { rank: number })[] = [];
  for (const mark of objectsIn<DocMark>(marks)) {
    const rank = MARK_ORDER.indexOf(mark.type);
    if (rank < 0 || drawn.some((other) => other.rank === rank)) continue;
    if (mark.type === 'link') {
      // Checked here as well as by the validator: this is where the address
      // becomes something that can be clicked.
      const href = mark.attrs?.href;
      if (isAllowedHref(href)) drawn.push({ rank, key: `link ${href}`, tag: 'a', href });
      continue;
    }
    const tag = MARK_TAGS[mark.type];
    if (tag) drawn.push({ rank, key: mark.type, tag });
  }
  return drawn.sort((a, b) => a.rank - b.rank);
}

/** Whether this is the document an empty body is stored as: one paragraph with nothing in it. */
function isEmptyDoc(doc: DocNode): boolean {
  const [only, ...others] = childrenOf(doc);
  return others.length === 0 && only?.type === 'paragraph' && childrenOf(only).length === 0;
}

function draw(doc: DocNode, options: RenderBodyOptions): DocumentFragment {
  const page = options.document ?? document;
  const isLanguage = NODE_SPECS.codeBlock?.attrs.language?.rule;

  const nameOf = (userId: string): string => options.resolveUser?.(userId) || userId;

  function mention(node: DocNode): HTMLElement {
    const userId = textAttribute(node, 'userId') ?? '';
    const element = page.createElement('span');
    element.setAttribute('data-user-id', userId);
    element.setAttribute('data-mention', '');
    element.setAttribute('class', 'mention');
    element.append(page.createTextNode(`@${nameOf(userId)}`));
    return element;
  }

  /**
   * The inline content of a paragraph, a heading or a code block. Texts that
   * follow each other share the elements of the marks they have in common,
   * from the outside in, the way the editor draws them.
   */
  function fillTextblock(element: HTMLElement, node: DocNode, marked: boolean): void {
    const open: { key: string; element: HTMLElement }[] = [];
    // Whether the last line has nothing on it yet. True for an empty block.
    let lineIsEmpty = true;
    for (const child of childrenOf(node)) {
      const value = typeof child.text === 'string' ? child.text : '';
      if (!INLINE.has(child.type) || (child.type === 'text' && value === '')) continue;
      const marks = marked ? drawnMarks(child.marks) : [];
      let keep = 0;
      while (keep < open.length && open[keep]?.key === marks[keep]?.key) keep += 1;
      open.length = keep;
      for (const mark of marks.slice(keep)) {
        const wrapper = page.createElement(mark.tag);
        if (mark.href !== undefined) {
          for (const [name, fixed] of Object.entries(LINK_ATTRIBUTES))
            wrapper.setAttribute(name, fixed);
          wrapper.setAttribute('href', mark.href);
        }
        (open.at(-1)?.element ?? element).append(wrapper);
        open.push({ key: mark.key, element: wrapper });
      }
      const parent = open.at(-1)?.element ?? element;
      if (child.type === 'text') {
        const last = parent.lastChild;
        if (last?.nodeType === TEXT_NODE) (last as Text).appendData(value);
        else parent.append(page.createTextNode(value));
        lineIsEmpty = value.endsWith('\n');
      } else if (child.type === 'hardBreak') {
        parent.append(page.createElement('br'));
        lineIsEmpty = true;
      } else if (child.type === 'mention') {
        parent.append(mention(child));
        lineIsEmpty = false;
      }
    }
    // A browser gives an empty last line no height: an empty paragraph would
    // collapse and a break at the end of a paragraph would not show. The
    // editor holds such a line open with a `<br>`, and so does this.
    if (lineIsEmpty) element.append(page.createElement('br'));
  }

  function textblock(tag: string, node: DocNode, marked = true): HTMLElement {
    const element = page.createElement(tag);
    fillTextblock(element, node, marked);
    return element;
  }

  function container(tag: string, node: DocNode): HTMLElement {
    const element = page.createElement(tag);
    for (const child of childrenOf(node)) {
      const drawn = block(child);
      if (drawn) element.append(drawn);
    }
    return element;
  }

  /** What a task says: the words of its first paragraph. */
  function taskText(node: DocNode): string {
    const words = (inline: DocNode): string => {
      if (inline.type === 'text') return typeof inline.text === 'string' ? inline.text : '';
      if (inline.type === 'hardBreak') return ' ';
      if (inline.type === 'mention') return `@${nameOf(textAttribute(inline, 'userId') ?? '')}`;
      return '';
    };
    const first = childrenOf(node)[0];
    return (first ? childrenOf(first) : []).map(words).join('').trim();
  }

  function taskItem(node: DocNode): HTMLElement {
    const checked = node.attrs?.checked === true;
    const item = page.createElement('li');
    item.setAttribute('data-checked', String(checked));

    // The box shows the state and reads it out, and cannot be changed: a
    // click (or the space bar, or a click on the label) is undone before it
    // takes effect. It is not `disabled`, which would grey it out and make
    // it look unlike the same task in the editor.
    const box = page.createElement('input');
    box.setAttribute('type', 'checkbox');
    box.checked = checked;
    box.setAttribute('aria-readonly', 'true');
    const text = taskText(node);
    const name = options.taskLabel ? options.taskLabel(text, checked) : text;
    if (name !== '') box.setAttribute('aria-label', name);
    box.addEventListener('click', (event) => {
      event.preventDefault();
    });

    const label = page.createElement('label');
    label.append(box);
    item.append(label, container('div', node));
    return item;
  }

  function image(node: DocNode): HTMLElement {
    const element = page.createElement('img');
    const fileId = textAttribute(node, 'fileId');
    const alt = textAttribute(node, 'alt');
    if (fileId !== undefined) element.setAttribute('data-file-id', fileId);
    if (alt !== undefined) element.setAttribute('alt', alt);
    // Before the address, so that the browser knows not to fetch at once.
    for (const [name, value] of Object.entries(IMAGE_ATTRIBUTES)) element.setAttribute(name, value);
    // Without an address the image still takes up room (editor.css), as in the editor.
    const src = fileId === undefined ? undefined : fileUrl(fileId);
    if (src) element.setAttribute('src', src);
    return element;
  }

  function block(node: DocNode): HTMLElement | undefined {
    switch (node.type) {
      case 'paragraph':
        return textblock('p', node);
      case 'heading':
        return textblock(headingTag(node), node);
      case 'bulletList':
        return container('ul', node);
      case 'orderedList': {
        const list = container('ol', node);
        const start = node.attrs?.start;
        if (typeof start === 'number' && Number.isInteger(start) && start !== 1)
          list.setAttribute('start', String(start));
        return list;
      }
      case 'listItem':
        return container('li', node);
      case 'taskList': {
        const list = container('ul', node);
        list.setAttribute('data-type', 'taskList');
        return list;
      }
      case 'taskItem':
        return taskItem(node);
      case 'blockquote':
        return container('blockquote', node);
      case 'codeBlock': {
        const pre = page.createElement('pre');
        // The text of a code block carries no marks.
        const code = textblock('code', node, false);
        const language = textAttribute(node, 'language');
        if (language !== undefined && isLanguage?.(language))
          code.setAttribute('class', `language-${language}`);
        pre.append(code);
        return pre;
      }
      case 'horizontalRule':
        return page.createElement('hr');
      case 'image':
        return image(node);
      default:
        // Not on the list: not drawn.
        return undefined;
    }
  }

  const fragment = page.createDocumentFragment();
  for (const child of childrenOf(doc)) {
    const drawn = block(child);
    if (drawn) fragment.append(drawn);
  }
  return fragment;
}

/**
 * The body as DOM nodes, or why it cannot be shown.
 *
 * `doc` is the stored document as it came from the server, already parsed
 * from JSON; `null` or `undefined` is an empty body, not a failure. A
 * document the validator refuses is not drawn, not even in part: the result
 * says so and the caller decides what to show in its place.
 */
export function renderBody(doc: unknown, options: RenderBodyOptions = {}): RenderedBody {
  const page = options.document ?? document;
  if (doc === null || doc === undefined) {
    return { ok: true, empty: true, fragment: page.createDocumentFragment() };
  }
  let problems: string[];
  try {
    problems = validateDoc(doc);
  } catch {
    // The validator expects the shape of a document and may trip over
    // something that has none, such as a number where a list should be.
    problems = ['not a document'];
  }
  if (problems.length > 0) return { ok: false, problems };

  const valid = doc as DocNode;
  if (isEmptyDoc(valid)) return { ok: true, empty: true, fragment: page.createDocumentFragment() };
  return { ok: true, empty: false, fragment: draw(valid, options) };
}
