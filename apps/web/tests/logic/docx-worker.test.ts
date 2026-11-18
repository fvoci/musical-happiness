import { describe, expect, it } from 'vitest';

import { HTML_LENGTH_LIMIT } from '../../src/viewer-frame/worker-html.ts';
import {
  footerAsHeaderDocx,
  manyListLevelsDocx,
  nestedSuperscriptDocx,
  notesInNotesDocx,
  styleChainDocx,
  wideTableHeaderDocx,
} from '../support/hostile-docx.ts';
import { DOCX_TEXT, koreanDocx, repeatedHeaderDocx } from '../support/viewer-files.ts';
import { drawAsWorker } from '../support/worker-globals.ts';

describe('the Word worker draws a document on its own DOM', () => {
  it('the Korean sample: its pages, header, table, picture and styles', async () => {
    const answer = await drawAsWorker(koreanDocx());
    if (answer.kind !== 'drawn') throw new Error(JSON.stringify(answer));
    expect(answer.body.match(/<section class="docx"/g)).toHaveLength(2);
    for (const text of [DOCX_TEXT.title, DOCX_TEXT.header, DOCX_TEXT.second, ...DOCX_TEXT.cells]) {
      expect(answer.body).toContain(text);
    }
    expect(answer.body).toMatch(/<img[^>]* src="TOKEN-0"/);
    expect(answer.images).toHaveLength(1);
    expect(answer.styles).toMatch(/^<style>/);
  });
});

describe('documents that make docx-preview do a great deal end within the limits', () => {
  // Each a few kilobytes; what the reviews of the viewer found. Without the worker's
  // ceilings each drew for minutes, or without end, on the frame's main thread.
  const cases: [string, () => Buffer, 'drawn' | 'too-large'][] = [
    [
      'runs in superscript nested 30 deep (drawn twice at each)',
      () => nestedSuperscriptDocx(30),
      'too-large',
    ],
    [
      'a header with a table of 5,000 columns, on 100 pages',
      () => wideTableHeaderDocx(5000, 100),
      'drawn',
    ],
    [
      '30 references to a footnote of 30 references to a long endnote',
      () => notesInNotesDocx(30, 30, 300),
      'drawn',
    ],
    [
      'a list of 300 levels in 300 numberings (a style rule each)',
      () => manyListLevelsDocx(300, 300),
      'too-large',
    ],
    [
      'a long footer that is the header too, on 50 pages',
      () => footerAsHeaderDocx(200, 50),
      'drawn',
    ],
    ['a chain of 20,000 styles', () => styleChainDocx(20_000), 'drawn'],
    ['a header of 2,000 paragraphs on 600 pages', () => repeatedHeaderDocx(600, 2000), 'too-large'],
  ];

  it.each(cases)(
    '%s',
    async (_, make, expected) => {
      const file = make();
      expect(file.length).toBeLessThan(200_000);
      const answer = await drawAsWorker(file);
      if (expected === 'too-large') {
        expect(answer).toEqual({ kind: 'failed', reason: 'too-large' });
        return;
      }
      if (answer.kind !== 'drawn') throw new Error(JSON.stringify(answer));
      expect(answer.styles.length + answer.body.length).toBeLessThanOrEqual(HTML_LENGTH_LIMIT);
    },
    20_000,
  );

  it('a list expanded past its ceiling is refused before docx-preview expands it', async () => {
    expect(await drawAsWorker(manyListLevelsDocx(400, 400))).toEqual({
      kind: 'failed',
      reason: 'too-large',
    });
  });
});
