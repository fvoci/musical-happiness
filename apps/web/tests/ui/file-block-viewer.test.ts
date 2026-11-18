/**
 * A file block whose file the viewer shows: its name is a button that opens
 * the viewer, in the editor and where a body is only shown, and Enter on the
 * selected block opens it too, leaving the document as it was; a file of
 * another kind, too large, or missing is only downloaded; a viewer that does
 * not come leaves the file to its download. The viewer opened from a block
 * (viewing.svelte.ts) gives the focus back to where it was pressed.
 */
import type { Editor } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { viewerChunk } from '$lib/components/file-viewer/chunk.svelte.ts';
import { viewerLinkOf, viewing } from '$lib/components/file-viewer/viewing.svelte.ts';
import BodyEditor from '$lib/editor/BodyEditor.svelte';
import BodyView from '$lib/editor/BodyView.svelte';
import {
  setFileSource,
  type FileDetails,
  type FileSource,
  type FileViewerLink,
} from '$lib/editor/files.ts';
import type { DocNode } from '$lib/editor/schema.ts';
import { opensInViewer, VIEW_LIMIT_BYTES } from '$lib/file-viewer/extensions.ts';

import { doc, paragraph, text } from '../support/body-docs.ts';

const fileBlock = (fileId: string): DocNode => ({ type: 'file', attrs: { fileId } });

const KNOWN: Record<string, FileDetails> = {
  '7minutes.pdf': { name: '회의록 2026-11.pdf', size: 2048 },
  '7plans.zip': { name: 'plans.zip', size: 10 },
  '7huge.pdf': { name: 'scan.pdf', size: VIEW_LIMIT_BYTES + 1 },
};

/** A viewer that opens whatever it is given, or says it did not come. */
function link(opened = true) {
  return {
    opens: vi.fn((details: FileDetails) => opensInViewer(details.name, details.size)),
    open: vi.fn((_fileId: string, _details: FileDetails, _back: () => HTMLElement | null) =>
      Promise.resolve(opened),
    ),
  } satisfies FileViewerLink;
}

function source(
  view: FileViewerLink | undefined,
  bytes = vi.fn(() => Promise.resolve(new Blob(['%PDF']))),
) {
  return {
    url: (fileId: string) => `/files/wiki-1/${fileId}`,
    details: (fileId: string) => Promise.resolve(KNOWN[fileId] ?? null),
    bytes,
    view,
  } satisfies FileSource;
}

let previous: FileSource;
beforeEach(() => {
  previous = setFileSource({ url: () => undefined });
});
afterEach(() => {
  setFileSource(previous);
  viewing.close();
  vi.restoreAllMocks();
});

function editorIn(container: HTMLElement): Editor {
  const root = container.querySelector<HTMLElement & { editor?: Editor }>('.ProseMirror');
  if (!root?.editor) throw new Error('no editor in the component');
  return root.editor;
}

function press(editor: Editor, key: string): boolean {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
  editor.view.dom.dispatchEvent(event);
  return event.defaultPrevented;
}

async function shown(container: HTMLElement): Promise<HTMLElement[]> {
  const blocks = [...container.querySelectorAll<HTMLElement>('[data-file-block]')];
  await vi.waitFor(() => {
    expect(blocks.map((block) => block.dataset.state)).not.toContain('pending');
  });
  return blocks;
}

describe('the name of a file the viewer shows', () => {
  it('is a button that opens the viewer, where a body is only shown', async () => {
    const viewer = link();
    setFileSource(source(viewer));
    const { container } = render(BodyView, { doc: doc(fileBlock('7minutes.pdf')) });
    const [block] = await shown(container);
    const button = block?.querySelector<HTMLButtonElement>('button.doc-file-open');
    expect(button?.textContent).toBe('회의록 2026-11.pdf');
    expect(button?.getAttribute('aria-label')).toBe('View 회의록 2026-11.pdf');
    expect(button?.type).toBe('button');
    expect(button?.tabIndex).toBe(0);
    // The download stays beside it.
    expect(block?.querySelector('a.doc-file-download')?.getAttribute('aria-label')).toBe(
      'Download 회의록 2026-11.pdf',
    );
    button?.click();
    expect(viewer.open).toHaveBeenCalledWith(
      '7minutes.pdf',
      KNOWN['7minutes.pdf'],
      expect.any(Function),
    );
    // The focus goes back to the button, or to the one drawn for the file in its place.
    const back = viewer.open.mock.lastCall?.[2];
    expect(back?.()).toBe(button);
    const redrawn = document.createElement('button');
    redrawn.className = 'doc-file-open';
    button?.replaceWith(redrawn);
    expect(back?.()).toBe(redrawn);
    redrawn.remove();
    expect(back?.()).toBeNull();
  });

  it('opens the viewer by Enter on the selected block in the editor, which stays as it was', async () => {
    const viewer = link();
    const bytes = vi.fn(() => Promise.resolve(new Blob(['%PDF'])));
    setFileSource(source(viewer, bytes));
    const body = doc(paragraph(text('a')), fileBlock('7minutes.pdf'), paragraph(text('b')));
    const { container } = render(BodyEditor, { doc: body });
    const editor = editorIn(container);
    await shown(container);
    editor.view.dispatch(editor.state.tr.setSelection(NodeSelection.create(editor.state.doc, 3)));
    expect(press(editor, 'Enter')).toBe(true);
    expect(viewer.open).toHaveBeenCalledTimes(1);
    expect(viewer.open.mock.lastCall?.[0]).toBe('7minutes.pdf');
    // Opened, not downloaded.
    expect(bytes).not.toHaveBeenCalled();
    expect(editor.getJSON()).toEqual(body);
    expect(editor.state.selection).toBeInstanceOf(NodeSelection);
  });

  it("keeps the keys pressed on it from the editor: they are the button's", async () => {
    const viewer = link();
    setFileSource(source(viewer));
    const body = doc(paragraph(text('a')), fileBlock('7minutes.pdf'), paragraph(text('b')));
    const { container } = render(BodyEditor, { doc: body });
    const editor = editorIn(container);
    const [block] = await shown(container);
    editor.view.dispatch(editor.state.tr.setSelection(NodeSelection.create(editor.state.doc, 3)));
    const button = block?.querySelector<HTMLButtonElement>('button.doc-file-open');
    for (const key of ['Enter', ' ', 'Backspace', 'Delete', 'a', 'ArrowDown']) {
      const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
      button?.dispatchEvent(event);
      // Not taken by the editor, which would have opened it, deleted the block or moved on.
      expect(event.defaultPrevented, key).toBe(false);
    }
    button?.dispatchEvent(
      new InputEvent('beforeinput', {
        inputType: 'insertText',
        data: ' ',
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(viewer.open).not.toHaveBeenCalled();
    expect(editor.getJSON()).toEqual(body);
    expect(editor.state.selection).toBeInstanceOf(NodeSelection);
  });

  it('is only text for a file of another kind, a file too large, and a missing one', async () => {
    const viewer = link();
    const bytes = vi.fn(() => Promise.resolve(new Blob(['x'])));
    setFileSource(source(viewer, bytes));
    const saved = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockReturnValue(undefined);
    const body = doc(fileBlock('7plans.zip'), fileBlock('7huge.pdf'), fileBlock('7gone.pdf'));
    const { container } = render(BodyEditor, { doc: body });
    const editor = editorIn(container);
    const blocks = await shown(container);
    expect(blocks.map((block) => block.querySelector('button'))).toEqual([null, null, null]);
    expect(blocks.map((block) => block.querySelector('.doc-file-name')?.tagName)).toEqual([
      'SPAN',
      'SPAN',
      'SPAN',
    ]);
    // Enter on the archive saves it, as its link does.
    editor.view.dispatch(editor.state.tr.setSelection(NodeSelection.create(editor.state.doc, 0)));
    expect(press(editor, 'Enter')).toBe(true);
    expect(saved).toHaveBeenCalledTimes(1);
    expect(viewer.open).not.toHaveBeenCalled();
  });

  it('is only text where the app has no viewer', async () => {
    setFileSource(source(undefined));
    const { container } = render(BodyView, { doc: doc(fileBlock('7minutes.pdf')) });
    const [block] = await shown(container);
    expect(block?.querySelector('button')).toBeNull();
  });

  it('leaves the file to its download when the viewer does not come', async () => {
    const viewer = link(false);
    const bytes = vi.fn(() => Promise.resolve(new Blob(['%PDF'])));
    setFileSource(source(viewer, bytes));
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:minutes');
    vi.spyOn(URL, 'revokeObjectURL').mockReturnValue(undefined);
    const { container } = render(BodyView, { doc: doc(fileBlock('7minutes.pdf')) });
    const [block] = await shown(container);
    block?.querySelector<HTMLButtonElement>('button.doc-file-open')?.click();
    await vi.waitFor(() => {
      expect(bytes).toHaveBeenCalledWith('7minutes.pdf');
    });
  });
});

describe('the viewer opened from a file block', () => {
  it('reads the file by its source, and gives the focus back to the editor or the button', async () => {
    vi.spyOn(viewerChunk, 'load').mockResolvedValue(true);
    const bytes = vi.fn((_fileId: string, _signal?: AbortSignal) =>
      Promise.resolve(new Blob(['%PDF'])),
    );
    const view = viewerLinkOf({ bytes });
    const details = KNOWN['7minutes.pdf'] ?? { name: '', size: 0 };
    expect(view?.opens(details)).toBe(true);
    expect(view?.opens({ name: 'plans.zip', size: 1 })).toBe(false);

    const editor = document.body.appendChild(document.createElement('div'));
    editor.setAttribute('contenteditable', 'true');
    editor.tabIndex = -1;
    const button = editor.appendChild(document.createElement('button'));
    expect(await view?.open('7minutes.pdf', details, () => button)).toBe(true);
    expect(viewing.file).toMatchObject({ name: '회의록 2026-11.pdf', size: 2048 });
    const signal = new AbortController().signal;
    await viewing.file?.read(signal);
    expect(bytes).toHaveBeenCalledWith('7minutes.pdf', signal);
    // A press of another file while one is open opens no second viewer, nor this one on it.
    expect(await view?.open('7plans.pdf', { name: 'plans.pdf', size: 1 }, () => button)).toBe(true);
    expect(viewing.file?.name).toBe('회의록 2026-11.pdf');
    viewing.close();
    expect(viewing.file).toBeUndefined();
    expect(document.activeElement).toBe(editor);

    const shownOnly = document.body.appendChild(document.createElement('button'));
    await view?.open('7minutes.pdf', details, () => shownOnly);
    viewing.close();
    expect(document.activeElement).toBe(shownOnly);

    // Let go of without the focus moving: the screen it was over is gone.
    button.focus();
    await view?.open('7minutes.pdf', details, () => shownOnly);
    viewing.forget();
    expect(viewing.file).toBeUndefined();
    expect(document.activeElement).toBe(button);
    editor.remove();
    shownOnly.remove();
  });

  it('is none for a source that cannot read bytes, and bytes that cannot be read are none', async () => {
    expect(viewerLinkOf({})).toBeUndefined();
    vi.spyOn(viewerChunk, 'load').mockResolvedValue(true);
    const view = viewerLinkOf({ bytes: () => Promise.reject(new Error('403')) });
    await view?.open('7minutes.pdf', { name: 'a.pdf', size: 1 }, () => document.body);
    expect(await viewing.file?.read()).toBeUndefined();
  });
});

describe('the files the viewer is likely to show', () => {
  it.each([
    ['a.pdf', 1, true],
    ['보고서.DOCX', 1, true],
    ['a.pdf', VIEW_LIMIT_BYTES, true],
    ['a.pdf', VIEW_LIMIT_BYTES + 1, false],
    ['a.zip', 1, false],
    ['pdf', 1, false],
    ['a.pdf.exe', 1, false],
    ['a.', 1, false],
  ])('%s of %i bytes: %s', (name, size, opens) => {
    expect(opensInViewer(name, size)).toBe(opens);
  });
});
