/**
 * The small box in which the source of a formula is written or changed.
 *
 * It opens under the formula (or under the line a new one is going into)
 * with the source in a text field and the drawn formula below it, redrawn as
 * the source changes; a source that cannot be drawn shows why instead. Plain
 * DOM, styled by editor.css: this layer cannot use the app's popover.
 *
 * Enter applies (in a block formula, where Enter makes a new line,
 * Ctrl+Enter or Cmd+Enter does), and so does moving on to something else.
 * Escape gives up the change. Applying an empty source removes the formula.
 */
import { m } from '$lib/i18n/paraglide/messages.js';

import { MATH_CLASS, MATH_SLOTS, MATH_SOURCE_ATTRIBUTE, type MathNodeName } from './drawing.ts';
import { fillMath } from './embeds.ts';
import { MAX_LATEX } from './schema.ts';

export interface MathEditingOptions {
  /** Which of the two kinds of formula. */
  node: MathNodeName;
  /** The source to start from; empty for a new formula. */
  latex: string;
  /** Where on the screen the formula is, now. Asked again when the window changes. */
  anchor: () => { left: number; top: number; bottom: number } | undefined;
  /** The source to keep; empty means "no formula". Called once, unless `cancel` is. */
  apply: (latex: string) => void;
  /** Nothing is to change. Called once, unless `apply` is. */
  cancel: () => void;
  page: Document;
}

let open: { apply: () => void; cancel: () => void } | undefined;

/** Opens the box. Only one is open at a time; opening another applies the first. */
export function editMath(options: MathEditingOptions): void {
  open?.apply();
  const { page } = options;
  const view = page.defaultView;
  const block = MATH_SLOTS[options.node].display;

  const box = page.createElement('div');
  box.setAttribute('class', 'doc-math-editing');
  box.setAttribute('role', 'dialog');
  box.setAttribute('aria-label', m.editor_math_edit());

  const field = page.createElement('textarea');
  field.setAttribute('aria-label', m.editor_math_source());
  field.setAttribute('maxlength', String(MAX_LATEX));
  field.setAttribute('rows', block ? '3' : '1');
  field.setAttribute('spellcheck', 'false');
  field.setAttribute('autocapitalize', 'off');
  field.setAttribute('autocomplete', 'off');
  field.value = options.latex;

  const preview = page.createElement('div');
  preview.setAttribute('class', 'doc-math-preview doc-body');
  preview.setAttribute('aria-live', 'polite');

  const hint = page.createElement('p');
  hint.setAttribute('class', 'doc-math-hint');
  hint.append(page.createTextNode(block ? m.editor_math_hint_block() : m.editor_math_hint()));

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
    const slot = page.createElement(MATH_SLOTS[options.node].tag);
    slot.setAttribute('class', MATH_CLASS);
    slot.setAttribute('data-type', MATH_SLOTS[options.node].type);
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
    const rect = options.anchor();
    if (!rect || !view) return;
    const size = box.getBoundingClientRect();
    const left = Math.max(8, Math.min(rect.left, view.innerWidth - size.width - 8));
    const below = rect.bottom + 6;
    const fits = below + size.height <= view.innerHeight || rect.top - 6 - size.height < 0;
    box.style.left = `${left}px`;
    box.style.top = `${fits ? below : rect.top - 6 - size.height}px`;
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
  field.addEventListener('keydown', (event) => {
    // While an input method is composing, its keys are not commands.
    if (event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    } else if (event.key === 'Enter' && (!block || event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      apply();
    }
  });
  button(m.ui_done(), 'apply', apply);
  button(m.editor_math_remove(), 'remove', () => {
    field.value = '';
    apply();
  });

  box.append(field, preview, hint, buttons);
  page.body.append(box);
  open = { apply, cancel };
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
