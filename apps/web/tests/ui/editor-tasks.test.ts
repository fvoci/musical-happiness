/**
 * The box of a task in the body editor (task-item.ts): a press changes the
 * task and nothing else. The editor takes no focus it did not have, the
 * change is one step of the document (one update of a shared one) that one
 * undo takes back, and a body that can only be read keeps its tasks.
 */
import type { Editor } from '@tiptap/core';
import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import * as Y from 'yjs';

import BodyEditor from '$lib/editor/BodyEditor.svelte';

import { doc, paragraph, task, tasks, text } from '../support/body-docs.ts';

const BODY = doc(
  paragraph(text('Before')),
  tasks(task(false, paragraph(text('Water the plants')))),
);

function editorIn(container: HTMLElement): Editor {
  const root = container.querySelector<HTMLElement & { editor?: Editor }>('.ProseMirror');
  if (!root?.editor) throw new Error('no editor in the component');
  return root.editor;
}

function partsOf(container: HTMLElement): { label: HTMLElement; box: HTMLInputElement } {
  const label = container.querySelector<HTMLElement>('ul[data-type="taskList"] > li > label');
  const box = label?.querySelector<HTMLInputElement>('input[type="checkbox"]');
  if (!label || !box) throw new Error('no task');
  return { label, box };
}

const checkedIn = (editor: Editor): unknown[] => {
  const found: unknown[] = [];
  editor.state.doc.descendants((node) => {
    if (node.type.name === 'taskItem') found.push(node.attrs.checked);
  });
  return found;
};

/** Counts the transactions that change the document from now on. */
function countChanges(editor: Editor): { count: number } {
  const seen = { count: 0 };
  editor.on('transaction', ({ transaction }) => {
    if (transaction.docChanged) seen.count += 1;
  });
  return seen;
}

describe('the box of a task', () => {
  it('changes the task in one step without focusing the editor, and one undo takes it back', () => {
    const { container } = render(BodyEditor, { doc: BODY });
    const editor = editorIn(container);
    const { label, box } = partsOf(container);
    const changes = countChanges(editor);

    // A press on the label is not one in the text: neither the browser nor the editor takes it.
    const pressed = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    expect(label.dispatchEvent(pressed)).toBe(false);
    box.click();

    expect(checkedIn(editor)).toEqual([true]);
    expect(box.checked).toBe(true);
    expect(changes.count).toBe(1);
    expect(editor.isFocused).toBe(false);
    expect(document.activeElement).not.toBe(editor.view.dom);

    editor.commands.undo();
    expect(checkedIn(editor)).toEqual([false]);
    expect(box.checked).toBe(false);
  });

  it('leaves the caret of a body being written where it was', () => {
    const { container } = render(BodyEditor, { doc: BODY });
    const editor = editorIn(container);
    editor.commands.focus(3, { scrollIntoView: false });
    const { from, to } = editor.state.selection;

    partsOf(container).box.click();

    expect(checkedIn(editor)).toEqual([true]);
    expect([editor.state.selection.from, editor.state.selection.to]).toEqual([from, to]);
  });

  it('passes a press on its label to the box, which keeps no focus from it', () => {
    const { container } = render(BodyEditor, { doc: BODY });
    const editor = editorIn(container);
    const { label, box } = partsOf(container);

    label.click();

    expect(checkedIn(editor)).toEqual([true]);
    expect(document.activeElement).not.toBe(box);
    expect(document.activeElement).not.toBe(editor.view.dom);
  });

  it('cannot be changed in a body that can only be read', () => {
    const { container } = render(BodyEditor, { doc: BODY, editable: false });
    const editor = editorIn(container);
    const { label, box } = partsOf(container);

    box.click();
    label.click();

    expect(checkedIn(editor)).toEqual([false]);
    expect(box.checked).toBe(false);
  });

  it('is one update of a shared document, which one undo takes back', async () => {
    const shared = new Y.Doc();
    const { container } = render(BodyEditor, { doc: null, collab: shared });
    const editor = editorIn(container);
    editor.commands.setContent(BODY);
    // The shared undo gathers changes close together into one step.
    await new Promise((resolve) => setTimeout(resolve, 600));
    let updates = 0;
    shared.on('update', () => {
      updates += 1;
    });

    partsOf(container).box.click();

    expect(updates).toBe(1);
    expect(checkedIn(editor)).toEqual([true]);
    editor.commands.undo();
    expect(checkedIn(editor)).toEqual([false]);
    expect(editor.state.doc.textContent).toBe('BeforeWater the plants');
  });
});
