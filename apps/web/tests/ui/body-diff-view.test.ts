/**
 * Two bodies merged by a comparison, drawn for reading (render-diff.ts): what
 * marks a block added, removed or changed, the words heard before each mark,
 * the notes of what a block was, the stops of the way through the changes,
 * and code whose lines changed keeping its marks through its colours
 * (code-view.ts). No stored body can carry the marks.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { answer } from '$lib/editor/code-highlight-worker.ts';
import { useHighlighter, type Highlighter } from '$lib/editor/code-highlighter.ts';
import { CODE_COPY_TEXTS } from '$lib/editor/code-copy-texts.ts';
import { drawCodeBlocks } from '$lib/editor/code-view.ts';
import { DIFF_STOP, drawDiffBlocks } from '$lib/editor/render-diff.ts';
import { renderBody } from '$lib/editor/render.ts';
import type { DocNode } from '$lib/editor/schema.ts';
import { diffBodies } from '$lib/logic/body-diff.ts';

import { bullets, code, doc, heading, image, item, paragraph, text } from '../support/body-docs.ts';
import { clipboardOf, codeRuns } from '../support/code-blocks.ts';

const inProcess: Highlighter = {
  ready: () => Promise.resolve(true),
  highlight: (grammar, value) => Promise.resolve(answer({ id: 0, grammar, text: value }).tokens),
  stop: () => undefined,
};

beforeEach(() => {
  useHighlighter(inProcess);
});
afterEach(() => {
  useHighlighter(undefined);
  document.body.replaceChildren();
});

/** What changed from `before` to `after`, drawn in the page as a `doc-body`. */
function drawn(before: DocNode, after: DocNode): HTMLElement {
  const root = document.createElement('div');
  root.className = 'doc-body';
  root.append(drawDiffBlocks(diffBodies(before, after).blocks));
  document.body.append(root);
  return root;
}

const task = (checked: boolean, value: string): DocNode => ({
  type: 'taskItem',
  attrs: { checked },
  content: [paragraph(text(value))],
});

describe('the marks of a comparison', () => {
  it('puts what was added and removed in <ins> and <del>, each with the words heard before it', () => {
    const root = drawn(doc(paragraph(text('the quick fox'))), doc(paragraph(text('the slow fox'))));
    const del = root.querySelector('p > del');
    const ins = root.querySelector('p > ins');
    expect([del?.textContent, ins?.textContent]).toEqual(['quick', 'slow']);
    expect(del?.getAttribute('data-change')).toBe('removed');
    expect(del?.getAttribute('data-diff-label')).toBeTruthy();
    expect(ins?.getAttribute('data-diff-label')).not.toBe(del?.getAttribute('data-diff-label'));
    // The paragraph is a stop of the way through the changes, and can take the focus.
    expect(root.querySelector('p')?.hasAttribute(DIFF_STOP)).toBe(true);
    expect(root.querySelector('p')?.getAttribute('tabindex')).toBe('-1');
  });

  it('marks a block added or removed whole, an image in an element that can carry the mark', () => {
    const root = drawn(
      doc(paragraph(text('kept')), image('f1')),
      doc(paragraph(text('kept')), paragraph(text('new'))),
    );
    const added = root.querySelector('p[data-diff="added"]');
    expect(added?.textContent).toBe('new');
    expect(added?.getAttribute('data-diff-label')).toBeTruthy();
    const removed = root.querySelector('[data-diff="removed"]');
    expect(removed?.tagName).toBe('DIV');
    expect(removed?.querySelector('img')?.getAttribute('data-file-id')).toBe('f1');
    expect(root.querySelectorAll(`[${DIFF_STOP}]`)).toHaveLength(2);
    // What is the same in both is drawn as it is, unmarked.
    expect(root.querySelector('p:not([data-diff])')?.textContent).toBe('kept');
  });

  it('says what a heading was and that a task was checked, the task keeping its box first', () => {
    const root = drawn(
      doc(heading(2, text('Plan')), { type: 'taskList', content: [task(false, 'call')] }),
      doc(heading(1, text('Plan')), { type: 'taskList', content: [task(true, 'call')] }),
    );
    const noted = root.querySelector('.doc-diff-noted');
    expect(noted?.querySelector('.doc-diff-note')?.textContent).toContain('→');
    expect(noted?.querySelector('h1')?.textContent).toBe('Plan');
    const checked = root.querySelector('li[data-checked="true"]');
    expect(checked?.firstElementChild?.tagName).toBe('LABEL');
    expect(checked?.lastElementChild?.firstElementChild?.className).toBe('doc-diff-note');
    expect(checked?.hasAttribute(DIFF_STOP)).toBe(true);
  });

  it('keeps a list a list: the item changed is marked in it', () => {
    const root = drawn(
      doc(bullets(item(paragraph(text('milk'))), item(paragraph(text('bread'))))),
      doc(bullets(item(paragraph(text('milk'))), item(paragraph(text('jam'))))),
    );
    expect(root.querySelectorAll('ul > li')).toHaveLength(2);
    expect(root.querySelector('ul > li:nth-child(2) del')?.textContent).toBe('bread');
  });
});

describe('a code block whose lines changed', () => {
  it('keeps its marks through its colours, copies the lines it has now, and numbers its lines', async () => {
    const root = drawn(
      doc(code('ts', 'const a = 1;\nconst b = 2;')),
      doc(code('ts', 'const a = 1;\nconst b = 3;')),
    );
    expect(root.querySelector('pre')?.getAttribute('data-diff')).toBe('changed');
    drawCodeBlocks(root, CODE_COPY_TEXTS);
    const written = clipboardOf(window);
    const codeElement = root.querySelector('.doc-code pre > code');
    await vi.waitFor(() => {
      expect(codeRuns(codeElement)).toContainEqual(['doc-code-keyword', 'const']);
    });
    // The old last line ends where the new one starts: a line between them, in neither.
    expect(codeElement?.querySelector('del')?.textContent).toBe('const b = 2;');
    expect(codeElement?.querySelector('[data-change="break"]')?.textContent).toBe('\n');
    expect(codeElement?.querySelector('ins')?.textContent).toBe('const b = 3;');
    expect(codeElement?.querySelector('ins .doc-code-keyword')?.textContent).toBe('const');
    expect(codeElement?.querySelector('ins')?.getAttribute('data-diff-label')).toBeTruthy();
    root.querySelector<HTMLButtonElement>('button.doc-code-copy')?.click();
    await vi.waitFor(() => {
      expect(written).toEqual(['const a = 1;\nconst b = 3;']);
    });
  });
});

describe('a stored body', () => {
  it('cannot carry the marks of a comparison: the validator refuses it', () => {
    const marked = doc(paragraph(text('x', { type: 'diff', attrs: { change: 'added' } })));
    expect(renderBody(marked).ok).toBe(false);
  });
});
