/**
 * Places held across what others write, in a body edited together
 * (shared-place.ts): an image going up lands where it was put however the
 * room changes the body above it or in its block, and is not put in (and
 * that said) when its block is deleted; the place of a block formula and
 * the box of a formula in a line follow their formula, and close when it
 * is deleted.
 */
import { getSchema, type Editor } from '@tiptap/core';
import { prosemirrorJSONToYXmlFragment } from '@tiptap/y-tiptap';
import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import * as Y from 'yjs';

import { BODY_FIELD } from '$lib/editor/body-editor.ts';
import BodyEditor from '$lib/editor/BodyEditor.svelte';
import { openPlace, placeOf } from '$lib/editor/editing-place.ts';
import { bodyExtensions } from '$lib/editor/extensions.ts';
import { setFileUrlResolver } from '$lib/editor/files.ts';
import type { DocNode } from '$lib/editor/schema.ts';

import { doc, math, mathBlock, paragraph, text } from '../support/body-docs.ts';

vi.mock('$lib/editor/diagram.ts', () => ({
  drawDiagram: () => Promise.resolve({ ok: false, reason: 'failed' }),
}));

setFileUrlResolver((fileId) => `/files/${fileId}`);

const schema = getSchema(bodyExtensions());

/** The server's document and the reader's, with the room between them. */
function room(body: DocNode): {
  server: Y.Doc;
  reader: Y.Doc;
  others: (change: () => void) => void;
} {
  const server = new Y.Doc();
  prosemirrorJSONToYXmlFragment(schema, body, server.getXmlFragment(BODY_FIELD));
  const reader = new Y.Doc();
  Y.applyUpdate(reader, Y.encodeStateAsUpdate(server));
  return {
    server,
    reader,
    /** Somebody else changes the server's document; the reader gets it, as from the room. */
    others(change) {
      Y.applyUpdate(server, Y.encodeStateAsUpdate(reader, Y.encodeStateVector(server)));
      const before = Y.encodeStateVector(server);
      change();
      Y.applyUpdate(reader, Y.encodeStateAsUpdate(server, before), 'room');
    },
  };
}

const paragraphOf = (value: string): Y.XmlElement => {
  const element = new Y.XmlElement('paragraph');
  element.insert(0, [new Y.XmlText(value)]);
  return element;
};

function editorIn(container: HTMLElement): Editor {
  const root = container.querySelector<HTMLElement & { editor?: Editor }>('.ProseMirror');
  if (!root?.editor) throw new Error('no editor in the component');
  return root.editor;
}

/** Pastes an image file the way a browser reports it. */
function pasteImage(editor: Editor): void {
  const event = new Event('paste', { bubbles: true, cancelable: true }) as Event & {
    clipboardData: { files: File[]; getData: () => string };
  };
  event.clipboardData = {
    files: [new File(['png'], 'a.png', { type: 'image/png' })],
    getData: () => '',
  };
  editor.view.someProp('handlePaste', (handle) =>
    handle(editor.view, event as unknown as ClipboardEvent, editor.state.doc.slice(0, 0)),
  );
}

/** The blocks of the editor's document, a word each. */
const blocks = (editor: Editor): string[] => {
  const found: string[] = [];
  editor.state.doc.forEach((node) => {
    found.push(node.type.name === 'paragraph' ? node.textContent : node.type.name);
  });
  return found;
};

/** An upload that ends when the test says so. */
function slowUpload(): { upload: (file: File) => Promise<string>; finish: () => void } {
  let finish = (): void => undefined;
  const upload = (): Promise<string> =>
    new Promise((resolve) => {
      finish = () => {
        resolve('file-1');
      };
    });
  return {
    upload,
    finish: () => {
      finish();
    },
  };
}

afterEach(() => {
  document.querySelectorAll('.doc-math-editing').forEach((node) => {
    node.remove();
  });
});

describe('an image going up in a body edited together', () => {
  it('lands where it was put, whatever the room writes above it and in its block', async () => {
    const shared = room(doc(paragraph(text('first')), paragraph(text('second'))));
    const { upload, finish } = slowUpload();
    const { container } = render(BodyEditor, {
      doc: null,
      collab: shared.reader,
      uploadImage: upload,
    });
    const editor = editorIn(container);
    // The caret at the end of "second".
    editor.commands.setTextSelection(editor.state.doc.content.size - 1);
    pasteImage(editor);
    shared.others(() => {
      const fragment = shared.server.getXmlFragment(BODY_FIELD);
      fragment.insert(0, [paragraphOf('above')]);
      // In the block itself, before the place: the editor redraws the whole block.
      ((fragment.get(2) as Y.XmlElement).get(0) as Y.XmlText).insert(0, 'the ');
    });
    expect(blocks(editor)).toEqual(['above', 'first', 'the second']);
    finish();
    await vi.waitFor(() => {
      expect(blocks(editor)).toEqual(['above', 'first', 'the second', 'image']);
    });
  });

  it('goes after its line when the room splits that line, writing a new one above the text', async () => {
    const shared = room(doc(paragraph(text('first')), paragraph(text('second'))));
    const { upload, finish } = slowUpload();
    const { container } = render(BodyEditor, {
      doc: null,
      collab: shared.reader,
      uploadImage: upload,
    });
    const editor = editorIn(container);
    editor.commands.setTextSelection(editor.state.doc.content.size - 1);
    pasteImage(editor);
    // Typed at the start of the line, then Enter: the binding keeps the old
    // block for the new first part and writes the rest anew after it.
    shared.others(() => {
      const fragment = shared.server.getXmlFragment(BODY_FIELD);
      const line = (fragment.get(1) as Y.XmlElement).get(0) as Y.XmlText;
      line.delete(0, line.length);
      line.insert(0, 'new line');
      fragment.insert(2, [paragraphOf('second')]);
    });
    expect(blocks(editor)).toEqual(['first', 'new line', 'second']);
    finish();
    await vi.waitFor(() => {
      expect(blocks(editor)).toEqual(['first', 'new line', 'second', 'image']);
    });
  });

  it('is not put in when the room deletes its block, and says so', async () => {
    const shared = room(doc(paragraph(text('first')), paragraph(text('second'))));
    const { upload, finish } = slowUpload();
    const { container } = render(BodyEditor, {
      doc: null,
      collab: shared.reader,
      uploadImage: upload,
    });
    const editor = editorIn(container);
    editor.commands.setTextSelection(editor.state.doc.content.size - 1);
    pasteImage(editor);
    shared.others(() => {
      shared.server.getXmlFragment(BODY_FIELD).delete(1, 1);
    });
    finish();
    await vi.waitFor(() => {
      expect(container.querySelector('.doc-editor-status')?.textContent.trim()).toBe(
        'The image was not put in: the text it was going into was removed.',
      );
    });
    expect(blocks(editor)).toEqual(['first']);
    expect(container.querySelector('.doc-upload')).toBeNull();
  });
});

describe('the place of a block formula in a body edited together', () => {
  it('follows its formula through what the room writes, and closes when the formula is deleted', () => {
    const shared = room(doc(paragraph(text('above')), mathBlock('x^2'), paragraph(text('below'))));
    const { container } = render(BodyEditor, { doc: null, collab: shared.reader });
    const editor = editorIn(container);
    const at = editor.state.doc.child(0).nodeSize;
    expect(openPlace(editor.view, at)).toBe(true);
    expect(placeOf(editor.state)).toMatchObject({ kind: 'formula', pos: at });

    shared.others(() => {
      shared.server.getXmlFragment(BODY_FIELD).insert(0, [paragraphOf('new')]);
    });
    const moved = at + editor.state.doc.child(0).nodeSize;
    expect(placeOf(editor.state)).toMatchObject({ kind: 'formula', pos: moved });
    expect(editor.state.doc.nodeAt(moved)?.type.name).toBe('blockMath');

    shared.others(() => {
      shared.server.getXmlFragment(BODY_FIELD).delete(2, 1);
    });
    expect(placeOf(editor.state)).toBeNull();
  });
});

describe('the box of a formula in a line in a body edited together', () => {
  const box = (): HTMLTextAreaElement | null =>
    document.querySelector<HTMLTextAreaElement>('.doc-math-editing textarea');

  it('writes into its formula after the room wrote above it', () => {
    const shared = room(doc(paragraph(text('a '), math('x'), text(' b'))));
    const { container } = render(BodyEditor, { doc: null, collab: shared.reader });
    const editor = editorIn(container);
    editor.view.dom.querySelector<HTMLElement>('.doc-math')?.click();
    const field = box();
    if (!field) throw new Error('no box');
    shared.others(() => {
      shared.server.getXmlFragment(BODY_FIELD).insert(0, [paragraphOf('new')]);
    });
    field.value = 'y';
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    let latex: unknown;
    editor.state.doc.descendants((node) => {
      if (node.type.name === 'inlineMath') latex = node.attrs.latex;
    });
    expect(latex).toBe('y');
    expect(blocks(editor)[0]).toBe('new');
  });

  it('closes, keeping nothing, when the room deletes the formula', () => {
    const shared = room(doc(paragraph(text('a '), math('x'), text(' b'))));
    const { container } = render(BodyEditor, { doc: null, collab: shared.reader });
    const editor = editorIn(container);
    editor.view.dom.querySelector<HTMLElement>('.doc-math')?.click();
    expect(box()).not.toBeNull();
    shared.others(() => {
      shared.server.getXmlFragment(BODY_FIELD).delete(0, 1);
    });
    expect(box()).toBeNull();
  });
});
