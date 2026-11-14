/**
 * Turns the markup of a drawn diagram into a picture that is safe to show.
 *
 * The diagram library takes text that one user wrote and gives back an SVG
 * picture as a string, which is then shown to other users. The library
 * cleans its own output, but what it lets through is decided there and may
 * change with a version. So the string is not trusted: it is read into a
 * document of its own, where nothing runs and nothing is fetched, and the
 * picture that goes into the page is built anew, element by element, from
 * the lists in this file. What is not on a list is not copied.
 *
 * What cannot get through, whatever the library does:
 *
 * - scripts, and any attribute that holds a script (`on…`);
 * - links (`<a>` is dropped; what it holds is kept);
 * - anything that names an address outside the picture: `<image>`,
 *   `<feImage>`, `href` and `xlink:href` other than `#name`, `url(…)` other
 *   than `url(#name)` in attributes and styles, `@import`, `@font-face`;
 * - `<foreignObject>` and with it every HTML element;
 * - style rules that reach outside the picture (every selector has to start
 *   with the identifier of the picture).
 *
 * THIS IS THE ONE FILE of the read-only view that turns a string into
 * elements (the parser below). tests/editor/render-imports.test.ts names it
 * as the exception and keeps every other file free of such calls.
 */

const SVG_NAMESPACE = 'http://www.w3.org/2000/svg';

/** What a picture may be made of. Everything else is dropped with what it holds. */
const ELEMENTS = new Set([
  'svg',
  'g',
  'defs',
  'symbol',
  'marker',
  'clipPath',
  'mask',
  'pattern',
  'linearGradient',
  'radialGradient',
  'stop',
  'path',
  'rect',
  'circle',
  'ellipse',
  'line',
  'polyline',
  'polygon',
  'text',
  'tspan',
  'textPath',
  'title',
  'desc',
  'switch',
  'use',
  'style',
  // Filters that work on the picture itself. `feImage` fetches and is not here.
  'filter',
  'feDropShadow',
  'feGaussianBlur',
  'feOffset',
  'feMerge',
  'feMergeNode',
  'feFlood',
  'feComposite',
  'feColorMatrix',
  'feBlend',
]);

/**
 * By the name a parser may give instead: one that does not know a newer
 * element leaves its name in lower case (`fedropshadow`).
 */
const ELEMENT_NAMES = new Map(Array.from(ELEMENTS, (name) => [name.toLowerCase(), name]));

/** Dropped, but what they hold is kept in their place: a link around a label. */
const UNWRAPPED = new Set(['a']);

/** The elements that may point at another element of the same picture, with `#name`. */
const REFERENCING = new Set(['use', 'textPath', 'linearGradient', 'radialGradient', 'pattern']);

const ATTRIBUTES = new Set([
  'class',
  'id',
  'role',
  'style',
  'xml:space',
  'data-look',
  // Geometry
  'viewBox',
  'preserveAspectRatio',
  'width',
  'height',
  'x',
  'y',
  'x1',
  'y1',
  'x2',
  'y2',
  'cx',
  'cy',
  'r',
  'rx',
  'ry',
  'dx',
  'dy',
  'd',
  'points',
  'transform',
  'transform-origin',
  'pathLength',
  'rotate',
  'textLength',
  'lengthAdjust',
  'startOffset',
  // Markers, gradients, patterns, clipping
  'markerWidth',
  'markerHeight',
  'markerUnits',
  'refX',
  'refY',
  'orient',
  'gradientUnits',
  'gradientTransform',
  'spreadMethod',
  'fx',
  'fy',
  'offset',
  'patternUnits',
  'patternContentUnits',
  'patternTransform',
  'clipPathUnits',
  'maskUnits',
  'maskContentUnits',
  // Filters
  'filterUnits',
  'primitiveUnits',
  'stdDeviation',
  'flood-color',
  'flood-opacity',
  'in',
  'in2',
  'result',
  'mode',
  'operator',
  'k1',
  'k2',
  'k3',
  'k4',
  'type',
  'values',
  // Presentation
  'fill',
  'fill-opacity',
  'fill-rule',
  'stroke',
  'stroke-width',
  'stroke-opacity',
  'stroke-dasharray',
  'stroke-dashoffset',
  'stroke-linecap',
  'stroke-linejoin',
  'stroke-miterlimit',
  'opacity',
  'color',
  'stop-color',
  'stop-opacity',
  'clip-path',
  'clip-rule',
  'mask',
  'filter',
  'marker-start',
  'marker-mid',
  'marker-end',
  'overflow',
  'visibility',
  'display',
  'vector-effect',
  'shape-rendering',
  'paint-order',
  // Text
  'font-family',
  'font-size',
  'font-style',
  'font-weight',
  'font-variant',
  'text-anchor',
  'text-decoration',
  'dominant-baseline',
  'alignment-baseline',
  'baseline-shift',
  'letter-spacing',
  'word-spacing',
  'writing-mode',
]);

/** A name that can be pointed at: letters, digits and a few signs, nothing that could end a `url(…)`. */
const NAME = /^[\p{L}\p{N}_][\p{L}\p{N}_.:-]{0,199}$/u;
/** `#name`, the only kind of reference there is. */
const LOCAL_REFERENCE = /^#[\p{L}\p{N}_][\p{L}\p{N}_.:-]{0,199}$/u;
/** `url(#name)`, bare or quoted. */
const LOCAL_URL = /url\(\s*(["']?)#[\p{L}\p{N}_][\p{L}\p{N}_.:-]{0,199}\1\s*\)/giu;

/**
 * The functions a style value or a presentation attribute may call. A
 * function that is not here (`image-set`, `src`, `element`, `attr`, …) makes
 * the whole value unusable; `url` is here and is checked separately.
 */
const CSS_FUNCTIONS = new Set([
  'rgb',
  'rgba',
  'hsl',
  'hsla',
  'var',
  'calc',
  'min',
  'max',
  'clamp',
  'translate',
  'translatex',
  'translatey',
  'rotate',
  'scale',
  'scalex',
  'scaley',
  'skewx',
  'skewy',
  'matrix',
  'drop-shadow',
  'brightness',
  'url',
]);

const MAX_DEPTH = 64;
const MAX_ELEMENTS = 50_000;
const MAX_ATTRIBUTE_LENGTH = 200_000;

/**
 * Whether a style value (or the value of a presentation attribute) can be
 * used as it is: it calls only the functions above, every `url(…)` in it
 * points into the picture, and it holds nothing that could hide either from
 * this check (an escape, a comment) or start something else (a block, an
 * at-rule, markup).
 */
export function isSafeCssValue(value: string): boolean {
  if (value.length > MAX_ATTRIBUTE_LENGTH) return false;
  if (/[\\<>@{}]/.test(value) || value.includes('/*')) return false;
  if (/javascript\s*:|expression/i.test(value)) return false;
  for (let at = value.indexOf('('); at !== -1; at = value.indexOf('(', at + 1)) {
    const name = /([\w-]+)\s*$/.exec(value.slice(0, at))?.[1];
    if (name === undefined || !CSS_FUNCTIONS.has(name.toLowerCase())) return false;
  }
  // Whatever `url(` is left once the references into the picture are taken out is one too many.
  return !/url\s*\(/i.test(value.replace(LOCAL_URL, ''));
}

/** Splits at a separator that is not inside quotes, brackets or parentheses. */
function splitTopLevel(text: string, separator: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote = '';
  let start = 0;
  for (let at = 0; at < text.length; at += 1) {
    const character = text.charAt(at);
    if (quote !== '') {
      if (character === quote) quote = '';
    } else if (character === '"' || character === "'") {
      quote = character;
    } else if (character === '(' || character === '[') {
      depth += 1;
    } else if (character === ')' || character === ']') {
      depth -= 1;
    } else if (character === separator && depth === 0) {
      parts.push(text.slice(start, at));
      start = at + 1;
    }
  }
  parts.push(text.slice(start));
  return parts;
}

/** The declarations of a block that can be used, as `[property, value]`. */
export function safeDeclarations(block: string): [string, string][] {
  const kept: [string, string][] = [];
  for (const declaration of splitTopLevel(block, ';')) {
    const colon = declaration.indexOf(':');
    if (colon === -1) continue;
    const property = declaration.slice(0, colon).trim();
    const value = declaration.slice(colon + 1).trim();
    if (!/^-{0,2}[A-Za-z][\w-]*$/.test(property) || value === '') continue;
    if (isSafeCssValue(value)) kept.push([property, value]);
  }
  return kept;
}

const serialise = (declarations: [string, string][]): string =>
  declarations.map(([property, value]) => `${property}:${value}`).join(';');

/** Whether a selector starts with the scope and goes on as a selector does. */
function isScoped(selector: string, scope: string): boolean {
  if (!selector.startsWith(scope)) return false;
  const next = selector.charAt(scope.length);
  return next === '' || /[\s.>:[+~]/.test(next);
}

/**
 * The rules of a style sheet that can be used. Kept are style rules whose
 * every selector starts with `scope` (so that nothing outside the picture is
 * styled), with the declarations that pass `safeDeclarations`, and keyframes.
 * Every other at-rule is dropped, `@import` and `@font-face` among them. A
 * sheet that holds an escape or markup is dropped whole: an escape can spell
 * a name this check looks for.
 */
export function safeStyleSheet(css: string, scope: string): string {
  if (css.includes('\\') || css.includes('<')) return '';
  const text = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const rules: string[] = [];
  let at = 0;

  /** The block that opens at `open`, without its braces, and where it ends. */
  function blockAt(open: number): { body: string; end: number } | undefined {
    let depth = 0;
    let quote = '';
    for (let index = open; index < text.length; index += 1) {
      const character = text.charAt(index);
      if (quote !== '') {
        if (character === quote) quote = '';
      } else if (character === '"' || character === "'") quote = character;
      else if (character === '{') depth += 1;
      else if (character === '}') {
        depth -= 1;
        if (depth === 0) return { body: text.slice(open + 1, index), end: index + 1 };
      }
    }
    return undefined;
  }

  while (at < text.length) {
    const open = text.indexOf('{', at);
    const semicolon = text.indexOf(';', at);
    if (open === -1) break;
    if (semicolon !== -1 && semicolon < open) {
      // A statement without a block (an at-rule that ends in a semicolon): skipped.
      at = semicolon + 1;
      continue;
    }
    const prelude = text.slice(at, open).trim();
    const block = blockAt(open);
    if (!block) break;
    at = block.end;

    if (prelude.startsWith('@')) {
      const keyframes = /^@(?:-webkit-)?keyframes\s+([A-Za-z_][\w-]*)$/.exec(prelude);
      if (!keyframes?.[1]) continue;
      const steps: string[] = [];
      let inner = 0;
      while (inner < block.body.length) {
        const stepOpen = block.body.indexOf('{', inner);
        const stepClose = block.body.indexOf('}', stepOpen);
        if (stepOpen === -1 || stepClose === -1) break;
        const step = block.body.slice(inner, stepOpen).trim();
        const declarations = safeDeclarations(block.body.slice(stepOpen + 1, stepClose));
        if (/^(from|to|\d{1,3}(\.\d+)?%)(\s*,\s*(from|to|\d{1,3}(\.\d+)?%))*$/.test(step)) {
          steps.push(`${step}{${serialise(declarations)}}`);
        }
        inner = stepClose + 1;
      }
      rules.push(`@keyframes ${keyframes[1]}{${steps.join('')}}`);
      continue;
    }

    // No nesting and no at-rules inside a style rule.
    if (block.body.includes('{') || block.body.includes('@')) continue;
    const selectors = splitTopLevel(prelude, ',').map((selector) => selector.trim());
    if (selectors.some((selector) => !isScoped(selector, scope))) continue;
    const declarations = safeDeclarations(block.body);
    if (declarations.length > 0) rules.push(`${selectors.join(',')}{${serialise(declarations)}}`);
  }
  return rules.join('\n');
}

export interface SafeSvgOptions {
  /** The document the picture is built in; the page's own unless given. */
  document?: Document | undefined;
}

/**
 * The picture, rebuilt from what is allowed; undefined when the markup is
 * not one SVG picture, or is too large or too deep to be one.
 */
export function safeSvg(markup: string, options: SafeSvgOptions = {}): SVGSVGElement | undefined {
  const page = options.document ?? document;
  // A document of its own: nothing in it runs, loads or is styled.
  const parsed = new DOMParser().parseFromString(markup, 'text/html');
  const root = parsed.body.firstElementChild;
  if (parsed.body.children.length !== 1 || !root) return undefined;
  if (root.localName !== 'svg' || root.namespaceURI !== SVG_NAMESPACE) return undefined;
  // Every direct child is counted, even when dropped, and the root counts too.
  // Refuse a tree that is already too wide before rebuilding any of its shapes.
  if (root.childElementCount >= MAX_ELEMENTS) return undefined;
  const rootId = root.getAttribute('id') ?? '';
  if (!NAME.test(rootId)) return undefined;
  const scope = `#${rootId}`;

  const seen = { count: 0, tooLarge: false };

  function copyAttributes(source: Element, target: Element): void {
    for (const { name, value } of Array.from(source.attributes)) {
      if (value.length > MAX_ATTRIBUTE_LENGTH) continue;
      if (name === 'href' || name === 'xlink:href') {
        // Only into the picture itself, and only where that means "use that element".
        if (REFERENCING.has(source.localName) && LOCAL_REFERENCE.test(value))
          target.setAttribute('href', value);
        continue;
      }
      if (/^aria-[a-z]+$/.test(name)) {
        target.setAttribute(name, value);
        continue;
      }
      if (!ATTRIBUTES.has(name)) continue;
      if (name === 'id') {
        if (NAME.test(value)) target.setAttribute('id', value);
      } else if (name === 'style') {
        // Through the style object, one property at a time, so that this
        // also works where a policy forbids style attributes set from markup.
        const style = (target as SVGElement).style;
        for (const [property, declared] of safeDeclarations(value)) {
          const important = /\s*!important$/i.test(declared);
          style.setProperty(
            property,
            declared.replace(/\s*!important$/i, ''),
            important ? 'important' : '',
          );
        }
      } else if (isSafeCssValue(value)) {
        target.setAttribute(name, value);
      }
    }
  }

  function copy(source: Element, depth: number): Node | undefined {
    if (seen.tooLarge) return undefined;
    seen.count += 1;
    if (seen.count > MAX_ELEMENTS || depth > MAX_DEPTH) {
      seen.tooLarge = true;
      return undefined;
    }
    if (source.namespaceURI !== SVG_NAMESPACE) return undefined;
    if (UNWRAPPED.has(source.localName)) {
      const kept = page.createDocumentFragment();
      copyChildren(source, kept, depth);
      return kept;
    }
    const name = ELEMENT_NAMES.get(source.localName.toLowerCase());
    if (name === undefined) return undefined;

    const target = page.createElementNS(SVG_NAMESPACE, name);
    copyAttributes(source, target);
    if (name === 'style') {
      const rules = safeStyleSheet(source.textContent, scope);
      if (rules === '') return undefined;
      target.append(page.createTextNode(rules));
      return target;
    }
    // A reference that points nowhere draws nothing; without it the element has no use.
    if (name === 'use' && !target.hasAttribute('href')) return undefined;
    copyChildren(source, target, depth);
    return target;
  }

  function copyChildren(source: Element, target: ParentNode, depth: number): void {
    for (const child of Array.from(source.childNodes)) {
      if (child.nodeType === 3) target.append(page.createTextNode(child.nodeValue ?? ''));
      else if (child.nodeType === 1) {
        const copied = copy(child as Element, depth + 1);
        if (copied) target.append(copied);
      }
      // Comments, processing instructions and the rest are not copied.
    }
  }

  const picture = copy(root, 1);
  if (seen.tooLarge || !picture) return undefined;
  return picture as SVGSVGElement;
}
