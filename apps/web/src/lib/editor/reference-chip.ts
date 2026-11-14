/**
 * How a reference is drawn, in the editor and where a body is only shown:
 * the icon and the title of what it names, as a link where the app has a
 * screen for it. A document holds the identifier alone, so the title is put
 * into the element here, from what the caller knows of what the reader may
 * see, and again whenever that changes.
 *
 * A reference is in one of three states (`data-state`): `pending` while what
 * it names is being asked, `shown` with a title, and `unseen` for what the
 * reader may not see or is not there. The last two never look alike, and
 * nothing of an unseen one tells which of the two it is.
 */
import { m } from '$lib/i18n/paraglide/messages.js';

import { REFERENCE_ID_ATTRIBUTE } from './drawing.ts';
import { isBlockId } from './schema.ts';

/** What a reference shows of what it names, to a reader who may see it. */
export interface ShownReference {
  title: string;
  /** Its icon; empty for none. */
  icon: string;
  kind: 'card' | 'page';
  /** The address it opens; none where the app has no screen for it. */
  href?: string;
}

/** What the references of a body are drawn from. The caller knows what the reader may see. */
export interface ReferenceDrawing {
  /**
   * What the reference of an identifier shows: `undefined` while that is being
   * asked, `null` for what the reader may not see. Read where the state the
   * caller keeps is followed, so that a title that arrives is drawn.
   */
  shown: (id: string) => ShownReference | null | undefined;
  /** Opens what a reference names, inside the app; without it a link opens as any link. */
  open?: (id: string) => void;
}

/** The elements references are drawn in, in the editor and in a body that is shown. */
export const REFERENCE_SELECTOR = 'span[data-reference]';
const LINK_MARK = 'data-reference-link';

type State = 'pending' | 'shown' | 'unseen';

function stateOf(
  id: string,
  drawing: ReferenceDrawing | undefined,
): [State, ShownReference | undefined] {
  // An identifier a reader by a link was not given is empty: nothing is asked for it.
  if (!drawing || !isBlockId(id)) return ['unseen', undefined];
  const shown = drawing.shown(id);
  if (shown === undefined) return ['pending', undefined];
  return shown === null ? ['unseen', undefined] : ['shown', shown];
}

function part(page: Document, name: string, text: string): HTMLElement {
  const element = page.createElement('span');
  element.setAttribute('class', name);
  element.textContent = text;
  return element;
}

/** Draws one reference into its element. Drawing the same again changes nothing. */
export function drawReference(element: HTMLElement, drawing: ReferenceDrawing | undefined): void {
  const id = element.getAttribute(REFERENCE_ID_ATTRIBUTE) ?? '';
  const [state, shown] = stateOf(id, drawing);
  const title = shown ? shown.title || m.editor_reference_untitled() : '';
  const drawn = JSON.stringify([state, shown?.icon, title, shown?.href, shown?.kind]);
  if (element.dataset.drawn === drawn) return;
  element.dataset.drawn = drawn;
  element.dataset.state = state;
  const page = element.ownerDocument;
  if (!shown) {
    element.removeAttribute('data-kind');
    if (state === 'pending') element.setAttribute('aria-busy', 'true');
    else element.removeAttribute('aria-busy');
    element.replaceChildren(state === 'pending' ? '…' : m.editor_reference_unseen());
    return;
  }
  element.removeAttribute('aria-busy');
  element.dataset.kind = shown.kind;
  const parts = [
    ...(shown.icon === '' ? [] : [part(page, 'reference-icon', shown.icon)]),
    part(page, 'reference-title', title),
  ];
  if (shown.href === undefined) {
    element.replaceChildren(...parts);
    return;
  }
  const link = page.createElement('a');
  link.setAttribute('href', shown.href);
  link.setAttribute(LINK_MARK, '');
  link.append(...parts);
  element.replaceChildren(link);
}

/** Draws every reference under `root`. */
export function drawReferences(root: ParentNode, drawing: ReferenceDrawing | undefined): void {
  for (const element of root.querySelectorAll<HTMLElement>(REFERENCE_SELECTOR)) {
    drawReference(element, drawing);
  }
}

/**
 * A click under `root`: on the link of a reference, plainly (no key held, the
 * main button), it opens what the reference names inside the app instead of
 * loading the page anew. Any other click is left to the browser.
 */
export function openClickedReference(
  event: MouseEvent,
  drawing: ReferenceDrawing | undefined,
): void {
  const open = drawing?.open;
  const target = event.target instanceof Element ? event.target : null;
  const link = target?.closest(`a[${LINK_MARK}]`);
  const id = link?.closest(REFERENCE_SELECTOR)?.getAttribute(REFERENCE_ID_ATTRIBUTE);
  if (!open || !id || event.defaultPrevented) return;
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return;
  }
  event.preventDefault();
  open(id);
}
