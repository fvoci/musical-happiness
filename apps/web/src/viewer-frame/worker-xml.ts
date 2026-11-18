/**
 * The XML half of the DOM docx-preview runs on in the Word worker
 * (`docx-worker.ts`), where browsers have no `DOMParser`: a parser with
 * namespaces (saxes) that builds the few node kinds docx-preview reads, with
 * the few members it reads (its `XmlParser`: `childNodes.item`,
 * `attributes.item`, `localName`, `namespaceURI`, `nodeType`, `textContent`,
 * `firstElementChild`; elsewhere `firstChild.nodeName` and
 * `lookupNamespaceURI`).
 *
 * Every node made counts toward one ceiling for the whole file, so that
 * parsing cannot hold more than a long document needs; past it the parser
 * throws `TooMuch`. A numbering part is checked too before docx-preview
 * expands it (each of its lists times the levels of the list it uses).
 */
import { SaxesParser, type SaxesTagNS } from 'saxes';

/** The most XML nodes a document's parts may have together: several times a long report's. */
export const XML_NODE_LIMIT = 3_000_000;
/** The most list levels docx-preview's expansion of a numbering part may make. */
export const NUMBERING_LIMIT = 100_000;

const WORDML = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';

/** Thrown when a document would make more than the worker takes on. */
export class TooMuch extends Error {}

/** A list docx-preview reads with `item(i)` and `length`, and `Array.from`. */
class ItemList<T> extends Array<T> {
  item(index: number): T | null {
    return this[index] ?? null;
  }
}

export class XmlAttr {
  readonly name: string;
  readonly localName: string;
  readonly namespaceURI: string | null;
  readonly value: string;

  constructor(name: string, localName: string, namespaceURI: string | null, value: string) {
    this.name = name;
    this.localName = localName;
    this.namespaceURI = namespaceURI;
    this.value = value;
  }
}

export class XmlText {
  readonly nodeType = 3;
  readonly nodeName = '#text';
  readonly data: string;

  constructor(data: string) {
    this.data = data;
  }

  get textContent(): string {
    return this.data;
  }
}

export class XmlElement {
  readonly nodeType = 1;
  readonly childNodes = new ItemList<XmlElement | XmlText>();
  readonly attributes = new ItemList<XmlAttr>();
  readonly nodeName: string;
  readonly localName: string;
  readonly namespaceURI: string | null;
  readonly parent: XmlElement | undefined;
  /** The namespaces this element declares, by prefix ('' for the default). */
  readonly #declared: Record<string, string>;

  constructor(
    nodeName: string,
    localName: string,
    namespaceURI: string | null,
    parent: XmlElement | undefined,
    declared: Record<string, string>,
  ) {
    this.nodeName = nodeName;
    this.localName = localName;
    this.namespaceURI = namespaceURI;
    this.parent = parent;
    this.#declared = declared;
  }

  get firstChild(): XmlElement | XmlText | null {
    return this.childNodes[0] ?? null;
  }

  get firstElementChild(): XmlElement | null {
    return this.childNodes.find((node) => node instanceof XmlElement) ?? null;
  }

  get textContent(): string {
    const parts: string[] = [];
    const stack: (XmlElement | XmlText)[] = [this];
    while (stack.length > 0) {
      const node = stack.pop();
      if (node instanceof XmlText) parts.push(node.data);
      else if (node) for (const child of node.childNodes.toReversed()) stack.push(child);
    }
    return parts.join('');
  }

  lookupNamespaceURI(prefix: string | null): string | null {
    const uri = this.#declared[prefix ?? ''];
    if (uri !== undefined) return uri === '' ? null : uri;
    return this.parent?.lookupNamespaceURI(prefix) ?? null;
  }
}

export class XmlDocument {
  readonly nodeType = 9;
  readonly firstElementChild: XmlElement | null;

  constructor(root: XmlElement | null) {
    this.firstElementChild = root;
  }

  /** docx-preview asks for "parsererror"; this parser throws instead of making one. */
  getElementsByTagName(): XmlElement[] {
    return [];
  }
}

/** Counts the nodes made, for the whole file, against `XML_NODE_LIMIT`. */
export class NodeCount {
  #made = 0;

  add(count = 1): void {
    this.#made += count;
    if (this.#made > XML_NODE_LIMIT) throw new TooMuch('XML nodes');
  }
}

/** How many list levels docx-preview would make of a numbering part. */
export function numberingSize(root: XmlElement): number {
  const levels = new Map<string, number>();
  const used: string[] = [];
  for (const child of root.childNodes) {
    if (!(child instanceof XmlElement)) continue;
    if (child.localName === 'abstractNum') {
      const id = child.attributes.find((each) => each.localName === 'abstractNumId')?.value ?? '';
      const count = child.childNodes.filter(
        (node) => node instanceof XmlElement && node.localName === 'lvl',
      ).length;
      levels.set(id, (levels.get(id) ?? 0) + count);
    } else if (child.localName === 'num') {
      const reference = child.childNodes.find(
        (node): node is XmlElement =>
          node instanceof XmlElement && node.localName === 'abstractNumId',
      );
      used.push(reference?.attributes.find((each) => each.localName === 'val')?.value ?? '');
    }
  }
  return used.reduce((sum, id) => sum + (levels.get(id) ?? 0), 0);
}

function element(tag: SaxesTagNS, parent: XmlElement | undefined): XmlElement {
  const made = new XmlElement(tag.name, tag.local, tag.uri || null, parent, tag.ns);
  for (const attribute of Object.values(tag.attributes)) {
    made.attributes.push(
      new XmlAttr(attribute.name, attribute.local, attribute.uri || null, attribute.value),
    );
  }
  return made;
}

/** Parses XML text into the nodes above; throws on text that is not well formed. */
export function parseXml(text: string, count: NodeCount): XmlDocument {
  const parser = new SaxesParser({ xmlns: true, position: false });
  let root: XmlElement | null = null;
  const open: XmlElement[] = [];
  parser.on('opentag', (tag) => {
    const parent = open.at(-1);
    const made = element(tag, parent);
    count.add(1 + made.attributes.length);
    if (parent) parent.childNodes.push(made);
    else root = made;
    open.push(made);
  });
  parser.on('closetag', () => {
    open.pop();
  });
  const text$ = (data: string): void => {
    const parent = open.at(-1);
    if (!parent) return;
    count.add();
    parent.childNodes.push(new XmlText(data));
  };
  parser.on('text', text$);
  parser.on('cdata', text$);
  parser.on('error', (error) => {
    throw error;
  });
  parser.write(text).close();
  const parsed = root as XmlElement | null;
  if (parsed?.localName === 'numbering' && parsed.namespaceURI === WORDML) {
    if (numberingSize(parsed) > NUMBERING_LIMIT) throw new TooMuch('list levels');
  }
  return new XmlDocument(parsed);
}

/** A `DOMParser` for docx-preview: XML only, every document counted against one ceiling. */
export function domParserFor(count: NodeCount): new () => {
  parseFromString(text: string, type: string): XmlDocument;
} {
  return class {
    parseFromString(text: string): XmlDocument {
      return parseXml(text, count);
    }
  };
}
