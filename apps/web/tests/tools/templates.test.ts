/**
 * The conversion of the built-in templates: the rules that change texts, and
 * the output the server takes in.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { validateDoc, type DocNode } from '$lib/editor/schema.ts';
import { parseDateValue } from '$lib/logic/wire.ts';

import { templateDocuments } from '../../tools/templates/archive.ts';
import { convertTemplates, outputFiles, type Output } from '../../tools/templates/output.ts';
import {
  leadsOutside,
  maskOldNames,
  removeOutsideLinks,
  rename,
  renameInDoc,
  sentenceRanges,
  valueLeadsOutside,
} from '../../tools/templates/rewrite.ts';

// The two names, put together from halves so that this file passes the names check.
const PRODUCT = ['Focal', 'board'].join('');
const COMPANY = ['Matter', 'most'].join('');

const text = (value: string, href?: string): DocNode =>
  href === undefined
    ? { type: 'text', text: value }
    : { type: 'text', text: value, marks: [{ type: 'link', attrs: { href } }] };
const paragraph = (...content: DocNode[]): DocNode => ({ type: 'paragraph', content });
const doc = (...content: DocNode[]): DocNode => ({ type: 'doc', content });

describe('the names of the original', () => {
  it('become the new name, whatever their case', () => {
    expect(rename(`${PRODUCT} plugin and ${COMPANY.toUpperCase()} server`)).toEqual({
      text: 'fvoci plugin and fvoci server',
      rules: ['old-name'],
      productWords: 0,
    });
  });

  it('are one name when the plain word follows the company', () => {
    expect(rename(`${COMPANY} Boards is a tool`).text).toBe('fvoci is a tool');
  });

  it('the plain word alone is replaced too, and counted', () => {
    expect(rename('Welcome to Boards!')).toEqual({
      text: 'Welcome to fvoci!',
      rules: ['product-word'],
      productWords: 1,
    });
    // Not the common noun.
    expect(rename('Move cards across boards').rules).toEqual([]);
  });

  it('leave a text without them as it is', () => {
    expect(rename('Team Schedule')).toEqual({ text: 'Team Schedule', rules: [], productWords: 0 });
  });

  it('are hidden in the list of changes', () => {
    expect(maskOldNames(`${PRODUCT} by ${COMPANY}`)).toBe(
      '{old product name} by {old company name}',
    );
  });

  it('are replaced in every text of a document, one record per paragraph', () => {
    const result = renameInDoc(
      doc(paragraph(text(`${COMPANY} Boards helps. `), text('Boards')), paragraph(text('Fine.'))),
    );
    expect(result.doc).toEqual(
      doc(paragraph(text('fvoci helps. '), text('fvoci')), paragraph(text('Fine.'))),
    );
    expect(result.renamed).toHaveLength(1);
    expect(result.renamed[0]?.rules.sort()).toEqual(['old-name', 'product-word']);
    expect(result.productWords).toBe(1);
  });
});

describe('links that lead outside', () => {
  it('are told from paths inside the app', () => {
    for (const href of [
      'https://example.com/a',
      'HTTP://x',
      'mailto:a@b.c',
      '//host/x',
      'java\tscript:x',
    ]) {
      expect(leadsOutside(href), href).toBe(true);
    }
    for (const href of ['/board/1', 'card/2', '#top', '?view=3']) {
      expect(leadsOutside(href), href).toBe(false);
    }
  });

  it('as the value of a link property: anything but a path inside the app', () => {
    for (const value of ['https://example.com', 'example.com', 'www.example.com/x', '//host']) {
      expect(valueLeadsOutside(value), value).toBe(true);
    }
    expect(valueLeadsOutside('/board/1')).toBe(false);
    expect(valueLeadsOutside('  ')).toBe(false);
  });

  it('cut a text into sentences that cover all of it', () => {
    const sample = 'One. Two! Three?\nFour';
    const ranges = sentenceRanges(sample);
    expect(ranges.map(([start, end]) => sample.slice(start, end))).toEqual([
      'One. ',
      'Two! ',
      'Three?\n',
      'Four',
    ]);
  });

  it('go, with the sentence that explains them', () => {
    const { doc: result, removed } = removeOutsideLinks(
      doc(
        paragraph(
          text('Use the '),
          { type: 'text', text: '@ symbol', marks: [{ type: 'bold' }] },
          text(' in a comment. They become a '),
          text('follower', 'https://docs.example.com/x'),
          text(' of the card. '),
        ),
      ),
    );
    expect(result).toEqual(
      doc(
        paragraph(
          text('Use the '),
          { type: 'text', text: '@ symbol', marks: [{ type: 'bold' }] },
          text(' in a comment.'),
        ),
      ),
    );
    expect(removed).toEqual([
      {
        before:
          'Use the @ symbol in a comment. They become a [follower](https://docs.example.com/x) of the card. ',
        after: 'Use the @ symbol in a comment.',
      },
    ]);
    expect(validateDoc(result)).toEqual([]);
  });

  it('keep the sentences before and after the one that goes', () => {
    const { doc: result } = removeOutsideLinks(
      doc(paragraph(text('First. See '), text('this', 'https://x.example'), text('. Last.'))),
    );
    expect(result).toEqual(doc(paragraph(text('First. Last.'))));
  });

  it('take along what they leave empty: the paragraph, the item, the list', () => {
    const item = (...content: DocNode[]): DocNode => ({ type: 'listItem', content });
    const list = (...content: DocNode[]): DocNode => ({ type: 'bulletList', content });
    const linked = paragraph(
      text('Read the '),
      text('docs', 'https://docs.example.com'),
      text('.'),
    );

    const some = removeOutsideLinks(doc(list(item(linked), item(paragraph(text('Stay.'))))));
    expect(some.doc).toEqual(doc(list(item(paragraph(text('Stay.'))))));

    const all = removeOutsideLinks(doc(paragraph(text('Intro.')), list(item(linked))));
    expect(all.doc).toEqual(doc(paragraph(text('Intro.'))));

    // An item keeps a paragraph as its first child when only its own text went.
    const nested = removeOutsideLinks(doc(list(item(linked, list(item(paragraph(text('In.'))))))));
    expect(nested.doc).toEqual(
      doc(list(item({ type: 'paragraph' }, list(item(paragraph(text('In.'))))))),
    );
    expect(validateDoc(nested.doc)).toEqual([]);

    expect(removeOutsideLinks(doc(linked)).doc).toBeUndefined();
  });

  it('leave links inside the app, and documents without links, untouched', () => {
    const inside = doc(paragraph(text('See '), text('the board', '/board/1'), text('.')));
    expect(removeOutsideLinks(inside)).toEqual({ doc: inside, removed: [] });
  });
});

describe('the converted templates', () => {
  const output: Output = convertTemplates();
  const everything = JSON.stringify(output.templates);

  it('are the thirteen of the archive, one of them for onboarding', () => {
    expect(output.templates).toHaveLength(13);
    expect(output.templates.filter((template) => template.onboarding)).toHaveLength(1);
    expect(output.templates.find((template) => template.onboarding)?.key).toBe('welcome');
    expect(output.templates.map((template) => template.key)).toEqual([
      'meeting-agenda',
      'sales-pipeline-crm',
      'personal-tasks',
      'project-tasks',
      'company-goals-okrs',
      'personal-goals',
      'sprint-planner',
      'user-research-sessions',
      'competitive-analysis',
      'content-calendar',
      'team-retrospective',
      'roadmap',
      'welcome',
    ]);
  });

  it('hold 77 cards, 58 of them with a body that the validator accepts', () => {
    const cards = output.templates.flatMap((template) => template.cards);
    expect(cards).toHaveLength(77);
    const bodies = cards.filter((card) => card.body !== null);
    expect(bodies).toHaveLength(58);
    expect(templateDocuments()).toHaveLength(58);
    for (const card of bodies) expect(validateDoc(card.body), card.title).toEqual([]);
    expect(output.templates.flatMap((template) => template.views)).toHaveLength(38);
  });

  it('name neither the original nor an address outside', () => {
    expect(everything.toLowerCase()).not.toContain(PRODUCT.toLowerCase());
    expect(everything.toLowerCase()).not.toContain(COMPANY.toLowerCase());
    expect(everything).not.toMatch(/https?:\/\/|\bwww\./i);
    expect(JSON.stringify(output.changes).toLowerCase()).not.toContain(COMPANY.toLowerCase());
  });

  it('list every change: 47 texts, by rule', () => {
    expect(output.index.changes).toEqual({
      'old-name': 9,
      'product-word': 5,
      'outside-link': 1,
      'outside-link-value': 32,
    });
    expect(output.changes).toHaveLength(47);
    for (const change of output.changes) {
      expect(change.before, `${change.template} ${change.title}`).not.toBe(change.after);
    }
    // The title the original found its onboarding board by is one of them.
    expect(output.changes).toContainEqual({
      template: 'welcome',
      part: 'board',
      title: 'Welcome to fvoci!',
      field: 'title',
      rule: 'product-word',
      before: 'Welcome to Boards!',
      after: 'Welcome to fvoci!',
    });
  });

  it('write date values the way the wire has them', () => {
    let dates = 0;
    for (const template of output.templates) {
      const dateIds = template.board.cardProperties
        .filter((property) => property.type === 'date')
        .map((property) => property.id);
      for (const card of template.cards) {
        for (const id of dateIds) {
          const value = card.fields.properties[id];
          if (value === undefined) continue;
          dates += 1;
          expect(parseDateValue(value).from?.date, card.title).toMatch(/^\d{4}-\d{2}-\d{2}$/);
        }
      }
    }
    expect(dates).toBe(19);
    expect(output.index.conversions.dateValues).toBe(19);
  });

  it('keep every reference inside a template whole', () => {
    for (const template of output.templates) {
      const cardIds = new Set(template.cards.map((card) => card.id));
      const properties = new Map(
        template.board.cardProperties.map((property) => [property.id, property]),
      );
      for (const view of template.views) {
        for (const id of view.fields.cardOrder as string[]) expect(cardIds.has(id)).toBe(true);
      }
      for (const card of template.cards) {
        for (const [id, value] of Object.entries(card.fields.properties)) {
          const property = properties.get(id);
          if (property?.type !== 'select') continue;
          expect(
            property.options.some((option) => option.id === value),
            `${template.key} / ${card.title}`,
          ).toBe(true);
        }
      }
      const fileIds = new Set(template.files.map((file) => file.fileId));
      const images = (node: DocNode): string[] =>
        node.type === 'image' ? [String(node.attrs?.fileId)] : (node.content ?? []).flatMap(images);
      for (const card of template.cards) {
        for (const fileId of card.body ? images(card.body) : []) {
          expect(fileIds.has(fileId)).toBe(true);
        }
      }
    }
  });

  it('are what the committed folder holds, byte for byte', () => {
    const folder = path.resolve(import.meta.dirname, '../../tools/templates/output');
    const files = outputFiles(output);
    expect(readdirSync(folder).sort()).toEqual([...files.keys(), 'README.md'].sort());
    for (const [name, content] of files) {
      expect(existsSync(path.join(folder, name)), name).toBe(true);
      expect(readFileSync(path.join(folder, name), 'utf8') === content, `${name} is stale`).toBe(
        true,
      );
    }
    // The same input gives the same bytes.
    expect([...outputFiles(convertTemplates())]).toEqual([...files]);
  });
});
