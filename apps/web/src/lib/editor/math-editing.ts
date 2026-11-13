/**
 * The small box in which the source of a formula in a line is changed. (A
 * block formula is changed in its place under it: math-block.ts.)
 *
 * It opens under the formula with the source in a text field and the drawn
 * formula below it, redrawn as the source changes; a source that cannot be
 * drawn shows why instead. Plain DOM, styled by editor.css: this layer cannot
 * use the app's popover.
 *
 * Enter applies, and so does moving on to something else. Escape gives up
 * the change. Applying an empty source removes the formula.
 */
import { m } from '$lib/i18n/paraglide/messages.js';

import { MATH_CLASS, MATH_SLOTS, MATH_SOURCE_ATTRIBUTE } from './drawing.ts';
import { fillMath } from './embeds.ts';
import { MAX_LATEX } from './schema.ts';

export interface MathEditingOptions {
  /** The source to start from. */
  latex: string;
  /** Where on the screen the formula is, now. Asked again when the window changes. */
  anchor: () => { left: number; top: number; bottom: number } | undefined;
  /** The source to keep; empty means "no formula". Called once, unless `cancel` is. */
  apply: (latex: string) => void;
  /** Nothing is to change. Called once, unless `apply` is. */
  cancel: () => void;
  page: Document;
  /** Whose box it is (the editor's view): for `applyMathEditing` and `mathDraftChanged`. */
  owner?: object;
}

/** Builds the dialog controls before any listeners for its lifetime are installed. */
function mathDialog(page: Document, options: MathEditingOptions) {
  const box = page.createElement('div');
  box.setAttribute('class', 'doc-math-editing');
  box.setAttribute('role', 'dialog');
  box.setAttribute('aria-label', m.editor_math_edit());

  const field = page.createElement('textarea');
  field.setAttribute('aria-label', m.editor_math_source());
  field.setAttribute('maxlength', String(MAX_LATEX));
  field.setAttribute('rows', '1');
  field.setAttribute('spellcheck', 'false');
  field.setAttribute('autocapitalize', 'off');
  field.setAttribute('autocomplete', 'off');
  field.value = options.latex;

  const preview = page.createElement('div');
  preview.setAttribute('class', 'doc-math-preview doc-body');
  preview.setAttribute('aria-live', 'polite');

  const hint = page.createElement('p');
  hint.setAttribute('class', 'doc-math-hint');
  hint.append(page.createTextNode(m.editor_math_hint()));

  const buttons = page.createElement('div');
  buttons.setAttribute('class', 'doc-math-buttons');
  const button = (label: string, kind: string, run: () => void): HTMLButtonElement => {
    const element = page.createElement('button');
    element.setAttribute('type', 'button');
    element.dataset.action = kind;
    element.append(page.createTextNode(label));
    element.addEventListener('click', run);
    buttons.append(element);
    return element;
  };

  return { box, field, preview, hint, buttons, button };
}

/** Places the dialog near its anchor, keeping it inside the window. */
function placeMathDialog(box: HTMLElement, options: MathEditingOptions, view: Window | null): void {
  const rect = options.anchor();
  if (!rect || !view) return;
  const size = box.getBoundingClientRect();
  const left = Math.max(8, Math.min(rect.left, view.innerWidth - size.width - 8));
  const below = rect.bottom + 6;
  const fits = below + size.height <= view.innerHeight || rect.top - 6 - size.height < 0;
  box.style.left = `${left}px`;
  box.style.top = `${fits ? below : rect.top - 6 - size.height}px`;
}

/** The keys of the field: Escape cancels, Enter keeps. */
function fieldKeys(apply: () => void, cancel: () => void): (event: KeyboardEvent) => void {
  return (event) => {
    // While an input method is composing, its keys are not commands.
    if (event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      apply();
    }
  };
}

let open:
  { apply: () => void; cancel: () => void; owner?: object; changed: () => boolean } | undefined;

/** Opens the box. Only one is open at a time; opening another applies the first. */
export function editMath(options: MathEditingOptions): void {
  open?.apply();
  const { page } = options;
  const view = page.defaultView;
  const { box, field, preview, hint, buttons, button } = mathDialog(page, options);

  let done = false;
  let sequence = 0;

  function redraw(): void {
    const latex = field.value.trim();
    sequence += 1;
    const mine = sequence;
    if (latex === '') {
      preview.replaceChildren();
      return;
    }
    // Drawn aside and swapped in when ready, so that the preview does not blink.
    const slot = page.createElement(MATH_SLOTS.inlineMath.tag);
    slot.setAttribute('class', MATH_CLASS);
    slot.setAttribute('data-type', MATH_SLOTS.inlineMath.type);
    slot.setAttribute(MATH_SOURCE_ATTRIBUTE, latex);
    slot.append(page.createTextNode(latex));
    const holder = page.createElement('div');
    holder.hidden = true;
    holder.append(slot);
    box.append(holder);
    void fillMath(slot)
      .catch(() => undefined)
      .then(() => {
        holder.remove();
        if (mine === sequence && !done) preview.replaceChildren(slot);
      });
  }

  function place(): void {
    placeMathDialog(box, options, view);
  }

  function close(): void {
    if (done) return;
    done = true;
    open = undefined;
    view?.removeEventListener('resize', place);
    page.removeEventListener('pointerdown', outside, true);
    box.remove();
  }
  function apply(): void {
    if (done) return;
    const latex = field.value.trim().slice(0, MAX_LATEX);
    close();
    options.apply(latex);
  }
  function cancel(): void {
    if (done) return;
    close();
    options.cancel();
  }
  /** A press anywhere else keeps what was written, like leaving a field. */
  function outside(event: Event): void {
    if (event.target instanceof Node && box.contains(event.target)) return;
    apply();
  }

  // Leaving the box for another part of the page keeps what was written.
  box.addEventListener('focusout', (event) => {
    const next = event.relatedTarget;
    if (next instanceof Node && !box.contains(next)) apply();
  });
  field.addEventListener('input', redraw);
  field.addEventListener('keydown', fieldKeys(apply, cancel));
  button(m.ui_done(), 'apply', apply);
  button(m.editor_math_remove(), 'remove', () => {
    field.value = '';
    apply();
  });

  box.append(field, preview, hint, buttons);
  page.body.append(box);
  open = {
    apply,
    cancel,
    owner: options.owner,
    changed: () => field.value.trim().slice(0, MAX_LATEX) !== options.latex,
  };
  place();
  redraw();
  view?.addEventListener('resize', place);
  page.addEventListener('pointerdown', outside, true);
  field.focus();
  field.select();
}

/**
 * Closes the box that is open, if one is, without keeping what was written:
 * for when the editor behind it goes away or is handed another document, and
 * a late change would land on something it was not written for.
 */
export function cancelMathEditing(): void {
  open?.cancel();
}

/** Closes the box of `owner`, if it is open, keeping what was written (as Done does). */
export function applyMathEditing(owner: object): void {
  if (open?.owner === owner) open.apply();
}

/** Whether the box of `owner` is open with a source other than the one it opened with. */
export function mathDraftChanged(owner: object): boolean {
  return open?.owner === owner && open.changed();
}
