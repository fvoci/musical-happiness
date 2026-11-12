/**
 * A formula on a line of its own, in the editor: its drawing, and the place
 * where its source is changed, opened right under the drawing in the flow of
 * the body (editing-place.ts), the way a diagram's is.
 *
 * Open, every change of the field goes into the document at once, so that it
 * is in the body's undo like any typing (Ctrl+Z in the field is the body's
 * undo), and the drawing above follows it. A source that cannot be drawn
 * leaves the last drawing dimmed, with what went wrong said under the field.
 * An emptied field keeps the last source in the document; closing the place
 * then removes the formula.
 *
 * A new formula (`$$` and a space on an empty line) has no node yet: a
 * formula without a source is none. Its place is drawn on that empty line,
 * with a preview above the field, and the formula goes in when the place
 * closes with something written.
 */
import type { Editor } from '@tiptap/core';
import type { Node as DocumentNode } from '@tiptap/pm/model';
import type { Decoration, NodeView } from '@tiptap/pm/view';

import { m } from '$lib/i18n/paraglide/messages.js';

import { placeFooter } from './diagram-block.ts';
import { mathSlotAttributes } from './drawing.ts';
import {
  closePlace,
  closesPlace,
  openPlace,
  placeIn,
  type NewFormulaPanel,
  type Place,
} from './editing-place.ts';
import { fillMath, mathProblem } from './embeds.ts';
import { MAX_LATEX } from './schema.ts';

const latexOf = (node: DocumentNode): string =>
  typeof node.attrs.latex === 'string' ? node.attrs.latex : '';

/** A fresh element for a block formula, holding its source until it is drawn. */
function formulaSlot(page: Document, latex: string): HTMLElement {
  const slot = page.createElement('div');
  for (const [name, value] of Object.entries(mathSlotAttributes('blockMath', latex)))
    slot.setAttribute(name, value);
  slot.append(page.createTextNode(latex));
  return slot;
}

/**
 * Draws a formula out of sight, inside `beside`, and gives the element it was
 * drawn into and whether it could be. Out of sight but laid out, so that a
 * formula far too tall is still caught.
 */
async function drawAside(
  beside: HTMLElement,
  latex: string,
): Promise<{ slot: HTMLElement; ok: boolean }> {
  const page = beside.ownerDocument;
  const slot = formulaSlot(page, latex);
  const holder = page.createElement('div');
  holder.setAttribute('class', 'doc-math-aside');
  holder.setAttribute('aria-hidden', 'true');
  holder.append(slot);
  beside.append(holder);
  await fillMath(slot).catch(() => undefined);
  holder.remove();
  return { slot, ok: slot.getAttribute('data-state') === 'drawn' };
}

/** The field of a place, with the keys that close it and the body's undo. */
function sourceField(page: Document, editor: Editor, latex: string): HTMLTextAreaElement {
  const field = page.createElement('textarea');
  field.setAttribute('aria-label', m.editor_math_source());
  field.setAttribute('maxlength', String(MAX_LATEX));
  field.setAttribute('rows', '3');
  field.setAttribute('spellcheck', 'false');
  field.setAttribute('autocapitalize', 'off');
  field.setAttribute('autocomplete', 'off');
  field.value = latex;
  field.addEventListener('keydown', (event) => {
    // While an input method is composing, its keys are not commands.
    if (event.isComposing) return;
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (mod && !event.altKey && (key === 'z' || key === 'y')) {
      // The field has no undo of its own: what it changed is in the body's.
      event.preventDefault();
      if (key === 'y' || event.shiftKey) editor.commands.redo();
      else editor.commands.undo();
    }
  });
  return field;
}

/** Says what went wrong under the field; the same words again are not said again. */
function say(problem: HTMLElement, parts: HTMLElement[]): void {
  const words = parts.map((part) => part.textContent).join(' ');
  if (problem.textContent !== words) problem.replaceChildren(...parts);
}

/** The box under a formula: the field, what went wrong (said aloud), how to close. */
function placeBox(page: Document, editor: Editor, field: HTMLTextAreaElement) {
  const problem = page.createElement('p');
  problem.setAttribute('class', 'doc-place-problem');
  problem.setAttribute('aria-live', 'polite');
  const box = page.createElement('div');
  box.setAttribute('class', 'doc-place');
  box.setAttribute('contenteditable', 'false');
  box.append(
    field,
    problem,
    placeFooter(
      page,
      () => {
        closePlace(editor.view, true);
      },
      m.editor_math_hint_block(),
    ),
  );
  // The keys that close the place work anywhere in it: the editor does not hear them here.
  box.addEventListener('keydown', (event) => {
    if (!closesPlace(event)) return;
    event.preventDefault();
    event.stopPropagation();
    closePlace(editor.view, true);
  });
  return { box, problem };
}

class FormulaView implements NodeView {
  readonly dom: HTMLElement;
  readonly #editor: Editor;
  readonly #getPos: () => number | undefined;
  #slot: HTMLElement;
  #latex: string;
  #place: Place | undefined;
  #panel: { box: HTMLElement; field: HTMLTextAreaElement; problem: HTMLElement } | undefined;
  /** Counts drawings, so that only the newest one is shown. */
  #drawn = 0;

  constructor(
    node: DocumentNode,
    editor: Editor,
    getPos: () => number | undefined,
    decorations: readonly Decoration[],
    page: Document,
  ) {
    this.#editor = editor;
    this.#getPos = getPos;
    this.#latex = latexOf(node);
    this.dom = page.createElement('div');
    this.dom.setAttribute('class', 'doc-math-block');
    this.#slot = formulaSlot(page, this.#latex);
    this.dom.append(this.#slot);
    this.#show();
    this.#setPlace(placeIn(decorations));
  }

  readonly #open = (): void => {
    const pos = this.#getPos();
    if (pos !== undefined && !this.#place) openPlace(this.#editor.view, pos);
  };

  /** Puts in a new element for the drawing, in place of the one there. */
  #replaceSlot(slot: HTMLElement): void {
    slot.addEventListener('click', this.#open);
    this.#slot.replaceWith(slot);
    this.#slot = slot;
  }

  /** Draws the stored source the way a closed formula shows it: a failure shows the source. */
  #show(): void {
    this.#drawn += 1;
    this.#replaceSlot(formulaSlot(this.dom.ownerDocument, this.#latex));
    // Without the library the source stays, which is what a failure looks like anyway.
    fillMath(this.#slot).catch(() => undefined);
  }

  /** While the place is open: the new drawing when it can be drawn, else the last one, dimmed. */
  async #preview(latex: string): Promise<void> {
    this.#drawn += 1;
    const mine = this.#drawn;
    const { slot, ok } = await drawAside(this.dom, latex);
    if (mine !== this.#drawn || !this.#panel) return;
    const { problem } = this.#panel;
    if (ok) {
      this.#replaceSlot(slot);
      problem.replaceChildren();
    } else {
      this.#slot.toggleAttribute('data-stale', this.#slot.getAttribute('data-state') === 'drawn');
      say(problem, mathProblem(this.dom.ownerDocument, slot));
    }
  }

  #setPlace(place: Place | undefined): void {
    this.#place = place;
    if (place && !this.#panel) {
      const page = this.dom.ownerDocument;
      const field = sourceField(page, this.#editor, this.#latex);
      const { box, problem } = placeBox(page, this.#editor, field);
      field.addEventListener('input', () => {
        this.#input(field.value);
      });
      this.dom.append(box);
      this.#panel = { box, field, problem };
      this.dom.setAttribute('data-open', '');
    } else if (!place && this.#panel) {
      this.#panel.box.remove();
      this.#panel = undefined;
      this.dom.removeAttribute('data-open');
      this.#show();
    }
    if (place?.draft && this.#panel) {
      const { field } = this.#panel;
      // What closing the place keeps: the field, which may be empty while the document is not.
      place.draft.value = () => field.value.trim().slice(0, MAX_LATEX);
    }
  }

  /** A change of the field goes into the document now, unless it is empty. */
  #input(value: string): void {
    const latex = value.trim().slice(0, MAX_LATEX);
    const pos = this.#getPos();
    if (latex === '' || pos === undefined || latex === this.#latex) return;
    const { view } = this.#editor;
    view.dispatch(view.state.tr.setNodeAttribute(pos, 'latex', latex));
  }

  update(node: DocumentNode, decorations: readonly Decoration[]): boolean {
    if (node.type.name !== 'blockMath') return false;
    const latex = latexOf(node);
    const changed = latex !== this.#latex;
    this.#latex = latex;
    const place = placeIn(decorations);
    if (place !== this.#place) this.#setPlace(place);
    if (changed && this.#panel) {
      // From the field, or from outside it (an undo): the field shows what is stored.
      if (this.#panel.field.value.trim() !== latex) this.#panel.field.value = latex;
      void this.#preview(latex);
    } else if (changed) this.#show();
    return true;
  }

  /** What happens in the box under the formula is not the editor's. */
  stopEvent(event: Event): boolean {
    return event.target instanceof Node && this.#panel?.box.contains(event.target) === true;
  }

  ignoreMutation(): boolean {
    // What changes inside is the drawing and the field, never the document.
    return true;
  }
}

/** The node view of a block formula. */
export function formulaBlockView(props: {
  node: DocumentNode;
  editor: Editor;
  getPos: () => number | undefined;
  decorations: readonly Decoration[];
}): NodeView {
  const { node, editor, getPos, decorations } = props;
  // The page the editor is in. (Its view is not there yet while the first nodes are drawn.)
  return new FormulaView(node, editor, getPos, decorations, document);
}

/** The place of a new formula, drawn on its empty line until the formula goes in. */
export function newFormulaPanel(editor: Editor): NewFormulaPanel {
  const page = editor.view.dom.ownerDocument;
  const preview = page.createElement('div');
  preview.setAttribute('class', 'doc-math-preview');
  const field = sourceField(page, editor, '');
  const { box, problem } = placeBox(page, editor, field);
  box.classList.add('doc-place-new');
  box.prepend(preview);
  let drawn = 0;
  field.addEventListener('input', () => {
    const latex = field.value.trim();
    drawn += 1;
    const mine = drawn;
    if (latex === '') {
      preview.replaceChildren();
      problem.replaceChildren();
      return;
    }
    void drawAside(box, latex).then(({ slot, ok }) => {
      if (mine !== drawn) return;
      if (ok) {
        preview.replaceChildren(slot);
        problem.replaceChildren();
      } else {
        preview.firstElementChild?.setAttribute('data-stale', '');
        say(problem, mathProblem(page, slot));
      }
    });
  });
  return {
    dom: box,
    value: () => field.value.trim().slice(0, MAX_LATEX),
    focus: () => {
      field.focus();
    },
  };
}
