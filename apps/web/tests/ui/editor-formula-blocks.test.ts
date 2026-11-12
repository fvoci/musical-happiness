/**
 * A formula on a line of its own, in the editor: its drawing alone until its
 * place is opened, the field right under the drawing while it is, and nothing
 * of the place in the document. Runs a real editor in a simulated page with
 * the real formula library, without layout.
 */
import { Editor } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { placeOf } from '$lib/editor/editing-place.ts';
import { bodyExtensions } from '$lib/editor/extensions.ts';
import { MAX_LATEX, toStoredDoc, validateDoc, type DocNode } from '$lib/editor/schema.ts';

import { doc, mathBlock, paragraph, text } from '../support/body-docs.ts';

vi.mock('$lib/editor/diagram.ts', () => ({
  drawDiagram: () => Promise.resolve({ ok: false, reason: 'failed' }),
}));

// Opening a place scrolls it into sight, which asks for the size of text: jsdom has none.
Range.prototype.getClientRects = function getClientRects() {
  return [] as unknown as DOMRectList;
};
Range.prototype.getBoundingClientRect = function getBoundingClientRect() {
  return new DOMRect();
};

let editor: Editor | undefined;
let updates = 0;

function create(content: DocNode, editable = true): Editor {
  const element = document.createElement('div');
  document.body.append(element);
  updates = 0;
  editor = new Editor({
    element,
    injectCSS: false,
    editable,
    extensions: bodyExtensions(),
    content,
    onUpdate: () => {
      updates += 1;
    },
  });
  return editor;
}

const stored = (target: Editor): DocNode => toStoredDoc(target.getJSON() as DocNode);
const block = (target: Editor): HTMLElement => {
  const found = target.view.dom.querySelector<HTMLElement>('.doc-math-block');
  if (!found) throw new Error('no block formula');
  return found;
};
const formula = (target: Editor): HTMLElement => {
  const found = block(target).querySelector<HTMLElement>(':scope > .doc-math');
  if (!found) throw new Error('no drawing');
  return found;
};
const field = (): HTMLTextAreaElement => {
  const found = document.querySelector<HTMLTextAreaElement>('.doc-place textarea');
  if (!found) throw new Error('no field');
  return found;
};
const isOpen = (target: Editor): boolean => block(target).hasAttribute('data-open');

/** A key pressed in the editor, the way the page hands it over. */
function press(target: Editor, init: KeyboardEventInit): void {
  target.view.dom.dispatchEvent(
    new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }),
  );
}
const pressIn = (element: HTMLElement, init: KeyboardEventInit): void => {
  element.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }));
};
function write(value: string): void {
  field().value = value;
  field().dispatchEvent(new Event('input', { bubbles: true }));
}

/** Types into the editor the way a keyboard does. */
function type(target: Editor, value: string): void {
  for (const character of value) {
    const { from, to } = target.state.selection;
    const insert = () => target.state.tr.insertText(character, from, to);
    const handled = target.view.someProp('handleTextInput', (handle) =>
      handle(target.view, from, to, character, insert),
    );
    if (!handled) target.view.dispatch(insert());
  }
}

const BODY = doc(paragraph(text('a')), mathBlock('x^2'), paragraph(text('b')));
const AT = 3;

afterEach(() => {
  editor?.destroy();
  editor = undefined;
  document.body.replaceChildren();
});

describe('a block formula', () => {
  it('is its drawing; a press opens the field under it, with the source', async () => {
    const target = create(BODY);
    await vi.waitFor(() => {
      expect(formula(target).getAttribute('data-state')).toBe('drawn');
    });
    expect(document.querySelector('.doc-place')).toBeNull();
    formula(target).click();
    expect(isOpen(target)).toBe(true);
    expect(placeOf(target.state)).toMatchObject({ kind: 'formula', pos: AT });
    expect(field().value).toBe('x^2');
    expect(document.activeElement).toBe(field());
    // Under the drawing, inside the body: no box floating over the page.
    expect(block(target).lastElementChild?.classList.contains('doc-place')).toBe(true);
    expect(document.querySelector('.doc-math-editing')).toBeNull();
  });

  it('opens on Enter while selected, and closes on Escape or Ctrl+Enter back on the block', () => {
    const target = create(BODY);
    target.view.dispatch(target.state.tr.setSelection(NodeSelection.create(target.state.doc, AT)));
    press(target, { key: 'Enter' });
    expect(isOpen(target)).toBe(true);
    const heard = vi.fn();
    document.addEventListener('keydown', heard);
    pressIn(field(), { key: 'Escape' });
    document.removeEventListener('keydown', heard);
    // Escape belonged to the place, not to what the editor is in.
    expect(heard).not.toHaveBeenCalled();
    expect(isOpen(target)).toBe(false);
    expect(target.state.selection).toBeInstanceOf(NodeSelection);
    expect(target.state.selection.from).toBe(AT);

    press(target, { key: 'Enter' });
    pressIn(field(), { key: 'Enter', ctrlKey: true });
    expect(isOpen(target)).toBe(false);
    expect(stored(target)).toEqual(BODY);
    expect(updates).toBe(0);
  });

  it('puts every change into the document at once, and into its undo', () => {
    const target = create(BODY);
    formula(target).click();
    write('y^3');
    expect(stored(target)).toEqual(
      doc(paragraph(text('a')), mathBlock('y^3'), paragraph(text('b'))),
    );
    // In the field, Ctrl+Z is the body's undo; the field shows what is stored again.
    pressIn(field(), { key: 'z', ctrlKey: true });
    expect(stored(target)).toEqual(BODY);
    expect(field().value).toBe('x^2');
    write('z');
    pressIn(field(), { key: 'Escape' });
    target.commands.undo();
    expect(stored(target)).toEqual(BODY);
    expect(isOpen(target)).toBe(false);
  });

  it('keeps the last drawing, dimmed, while the source cannot be drawn, and says why under the field', async () => {
    const target = create(BODY);
    await vi.waitFor(() => {
      expect(formula(target).getAttribute('data-state')).toBe('drawn');
    });
    formula(target).click();
    write('\\frac{');
    await vi.waitFor(() => {
      expect(formula(target).hasAttribute('data-stale')).toBe(true);
    });
    expect(formula(target).getAttribute('data-state')).toBe('drawn');
    const problem = block(target).querySelector('.doc-place-problem');
    expect(problem?.getAttribute('aria-live')).toBe('polite');
    expect(problem?.textContent).toBe('This formula cannot be drawn.');
    // Closed, a source that cannot be drawn shows as itself, with why.
    pressIn(field(), { key: 'Escape' });
    await vi.waitFor(() => {
      expect(formula(target).getAttribute('data-state')).toBe('error');
    });
    expect(formula(target).textContent).toContain('\\frac{');
  });

  it('removes the formula when it is closed empty, and keeps a source to the limit', () => {
    const target = create(BODY);
    formula(target).click();
    write('   ');
    // Nothing empty goes into the document meanwhile.
    expect(stored(target)).toEqual(BODY);
    pressIn(field(), { key: 'Escape' });
    expect(stored(target)).toEqual(doc(paragraph(text('a')), paragraph(text('b'))));

    const second = create(doc(mathBlock('x')));
    formula(second).click();
    write('y'.repeat(MAX_LATEX + 50));
    expect(stored(second)).toEqual(doc(mathBlock('y'.repeat(MAX_LATEX))));
    expect(validateDoc(stored(second))).toEqual([]);
  });

  it('closes on a press elsewhere, keeping what was written', () => {
    const target = create(BODY);
    formula(target).click();
    write('kept');
    document.body.click();
    expect(isOpen(target)).toBe(false);
    expect(stored(target)).toEqual(
      doc(paragraph(text('a')), mathBlock('kept'), paragraph(text('b'))),
    );
  });

  it('does not open where the body cannot be edited', () => {
    const target = create(BODY, false);
    formula(target).click();
    expect(document.querySelector('.doc-place')).toBeNull();
  });
});

describe('a new block formula', () => {
  it('opens its place on its empty line on `$$` and a space, and goes in when closed', async () => {
    const target = create(doc(paragraph()));
    type(target, '$$ ');
    // Nothing is stored for a formula that has no source yet.
    expect(stored(target)).toEqual(doc(paragraph()));
    const box = document.querySelector<HTMLElement>('.doc-place-new');
    expect(box?.closest('p')).not.toBeNull();
    expect(document.activeElement).toBe(field());
    expect(field().getAttribute('aria-label')).toBe('LaTeX source');
    write('a^2 + b^2');
    await vi.waitFor(() => {
      expect(box?.querySelector('.doc-math-preview .katex')).not.toBeNull();
    });
    pressIn(field(), { key: 'Enter', ctrlKey: true });
    expect(document.querySelector('.doc-place-new')).toBeNull();
    expect(stored(target)).toEqual(doc(mathBlock('a^2 + b^2'), paragraph()));
    // Selected, the editor has the focus back.
    expect(target.state.selection).toBeInstanceOf(NodeSelection);
    // An undo takes it out again.
    target.commands.undo();
    expect(JSON.stringify(stored(target))).not.toContain('blockMath');
  });

  it('leaves the line as it was when closed with nothing written', () => {
    const target = create(doc(paragraph()));
    type(target, '$$ ');
    pressIn(field(), { key: 'Escape' });
    expect(document.querySelector('.doc-place-new')).toBeNull();
    expect(stored(target)).toEqual(doc(paragraph()));
  });
});
