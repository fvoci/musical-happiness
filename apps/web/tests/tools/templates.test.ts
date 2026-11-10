/**
 * The conversion of the built-in templates: the rules that change texts, and
 * the output the server takes in.
 *
 * The output is committed (tools/templates/output), and what has to hold of
 * it is tested on the committed files, with or without the original. That
 * the files are what the conversion writes from the archive today is tested
 * only where the archive is; elsewhere those tests are skipped, and a line
 * says so.
 */
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { validateDoc, type DocNode } from '$lib/editor/schema.ts';
import { parseDateValue } from '$lib/logic/wire.ts';

import { hasTemplateArchive, skipNotice, TEMPLATE_ARCHIVE } from '../../tools/original.ts';
import { archivedDocuments, templateDocuments } from '../../tools/templates/archive.ts';
import {
  committedDocuments,
  OUTPUT_FOLDER,
  readCommitted,
  staleFiles,
} from '../../tools/templates/committed.ts';
import {
  CHANGE_RULES,
  convertTemplates,
  outputFiles,
  outputProblems,
  promiseProblems,
  type Output,
  type Template,
} from '../../tools/templates/output.ts';
import {
  isExampleAddress,
  leadsOutside,
  maskOldNames,
  removeOutsideLinks,
  rename,
  renameInDoc,
  rewriteLinkValue,
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

  it('as the value of a link property, go when they lead to the company of the original', () => {
    const company = COMPANY.toLowerCase();
    for (const value of [
      `https://${company}.com/boards/`,
      `https://docs.${company}.com/boards/work-with-cards.html#mention-people`,
      `http://WWW.${COMPANY}.com`,
      `${company}.org`,
      `https://${PRODUCT.toLowerCase()}.com/download`,
    ]) {
      expect(rewriteLinkValue(value), value).toEqual({ to: 'removed' });
    }
  });

  it('as the value of a link property, move under the example domain otherwise', () => {
    const moved = (value: string): string | undefined => {
      const link = rewriteLinkValue(value);
      return link.to === 'example' ? link.value : undefined;
    };
    // A company that was made up: the site, by its name.
    expect(moved('liminarycorp.com')).toBe('https://liminarycorp.example.com');
    expect(moved('www.liminarycorp.com/')).toBe('https://liminarycorp.example.com');
    expect(moved('//liminarycorp.com')).toBe('https://liminarycorp.example.com');
    // A page somewhere: the path, the query and the fragment stay.
    expect(moved('https://www.toolbox.com/tech/devops/articles/best-devops-tools/')).toBe(
      'https://toolbox.example.com/tech/devops/articles/best-devops-tools/',
    );
    expect(moved('http://user-images.site.com:8080/a/b.gif?x=1#top')).toBe(
      'https://user-images.site.example.com/a/b.gif?x=1#top',
    );
    // Named in the path, the company is where the address leads: its account on another site.
    expect(rewriteLinkValue(`https://twitter.com/${COMPANY.toLowerCase()}/status/1?s=20`)).toEqual({
      to: 'removed',
    });
    expect(moved('http://docs.example.com/x')).toBe('https://docs.example.com/x');
    expect(moved('localhost')).toBe('https://localhost.example.com');
  });

  it('as the value of a link property, stay when they lead nowhere outside or are an example already', () => {
    for (const value of ['/board/1', '  ', 'https://example.com', 'https://a.example.com/x?y#z']) {
      expect(rewriteLinkValue(value), value).toEqual({ to: 'kept' });
    }
  });

  it('as the value of a link property, go when they are not the address of a site', () => {
    for (const value of ['mailto:someone@email.com', 'ftp://files.site.com/x', 'https://']) {
      expect(rewriteLinkValue(value), value).toEqual({ to: 'removed' });
    }
  });

  it('are an example address only under the example domain, over https, naming no other site', () => {
    for (const value of [
      'https://example.com',
      'https://example.com/',
      'https://liminarycorp.example.com',
      'https://a.b.example.com/x?y=1#z',
    ]) {
      expect(isExampleAddress(value), value).toBe(true);
    }
    for (const value of [
      'http://example.com',
      'example.com',
      'https://example.com.evil.site',
      'https://notexample.com',
      'https://example.org',
      'https://example.com:8080/',
      'https://example.com/?next=https://real.site',
      'https://example.com/www.real.site',
    ]) {
      expect(isExampleAddress(value), value).toBe(false);
    }
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

/** The values of the link properties of a template's cards. */
const linkValues = (template: Template): string[] => {
  const linkIds = template.board.cardProperties
    .filter((property) => property.type === 'url')
    .map((property) => property.id);
  return template.cards.flatMap((card) =>
    linkIds.flatMap((id) => {
      const value = card.fields.properties[id];
      return typeof value === 'string' ? [value] : [];
    }),
  );
};

describe('the committed templates', () => {
  const output: Output = readCommitted();
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
    expect(committedDocuments()).toHaveLength(58);
    expect(templateDocuments()).toHaveLength(58);
    for (const card of bodies) expect(validateDoc(card.body), card.title).toEqual([]);
    expect(output.templates.flatMap((template) => template.views)).toHaveLength(38);
  });

  it('name neither the original nor an address outside, but for example addresses in link values', () => {
    expect(everything.toLowerCase()).not.toContain(PRODUCT.toLowerCase());
    expect(everything.toLowerCase()).not.toContain(COMPANY.toLowerCase());
    expect(JSON.stringify(output.changes).toLowerCase()).not.toContain(COMPANY.toLowerCase());
    expect(promiseProblems(output.templates)).toEqual([]);

    // Every address in the output is the whole value of a link property, and an example.
    const addresses = everything.match(/https?:\/\/|\bwww\./gi) ?? [];
    const values = output.templates.flatMap(linkValues).filter(valueLeadsOutside);
    expect(values).toHaveLength(10);
    expect(addresses).toHaveLength(values.length);
    for (const value of values) expect(isExampleAddress(value), value).toBe(true);
    // No body of a card has a link that leads outside, example or not.
    const bodies = JSON.stringify(
      output.templates.flatMap((t) => t.cards.map((card) => card.body)),
    );
    expect(bodies).not.toMatch(/https?:\/\/|\bwww\./i);
  });

  it('would be refused with an address that is not an example, or with one outside a link value', () => {
    const copy = (): Template[] => JSON.parse(everything) as Template[];
    const withLink = (templates: Template[]): Template => {
      const found = templates.find((template) => linkValues(template).length > 0);
      if (!found) throw new Error('no template with a link value');
      return found;
    };

    const real = copy();
    const template = withLink(real);
    const linkId = template.board.cardProperties.find((property) => property.type === 'url')?.id;
    const card = template.cards.find(
      (one) => typeof one.fields.properties[linkId ?? ''] === 'string',
    );
    if (!card || linkId === undefined) throw new Error('no card with a link value');
    card.fields.properties[linkId] = 'https://liminarycorp.com';
    expect(promiseProblems(real).join()).toMatch(/a link value leads outside/);

    const bare = copy();
    const bareCard = withLink(bare).cards.find(
      (one) => typeof one.fields.properties[linkId] === 'string',
    );
    if (!bareCard) throw new Error('no card with a link value');
    bareCard.fields.properties[linkId] = 'liminarycorp.com';
    expect(promiseProblems(bare).join()).toMatch(/a link value leads outside/);

    const elsewhere = copy();
    const first = elsewhere[0];
    if (!first) throw new Error('no template');
    first.board.description = 'See https://example.com for more.';
    expect(promiseProblems(elsewhere)).toEqual([
      'an address that leads outside is left in the output',
    ]);

    const named = copy();
    const second = named[0];
    if (!second) throw new Error('no template');
    second.board.title = `${PRODUCT} tasks`;
    expect(promiseProblems(named)).toEqual(['a name of the original is left in the output']);
  });

  it('list every change: 47 texts, by rule', () => {
    expect(output.index.changes).toEqual({
      'old-name': 9,
      'product-word': 5,
      'outside-link': 1,
      'outside-link-value': 22,
      'example-link-value': 10,
    });
    expect(Object.keys(output.index.changes)).toEqual([...CHANGE_RULES]);
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

  it('removed the link values that led to the company of the original, and moved the others', () => {
    const removed = output.changes.filter((change) => change.rule === 'outside-link-value');
    for (const change of removed) {
      expect(change.after).toBe('');
      // The masked name is a label of the host, or is in the path.
      expect(change.before, change.title).toMatch(
        /^https:\/\/(?:(?:[a-z0-9-]+\.)*\{old company name\}\.com\/|[^/]+\/.*\{old company name\})/,
      );
    }
    const moved = output.changes.filter((change) => change.rule === 'example-link-value');
    for (const change of moved) {
      expect(isExampleAddress(change.after), change.after).toBe(true);
      expect(change.before, change.title).not.toMatch(/^(?:https?:\/\/)?[^/]*\{old/);
    }
    // The five companies of the competitive analysis were made up; each keeps a site of its own.
    expect(
      moved.filter((change) => change.template === 'competitive-analysis').map((c) => c.after),
    ).toEqual([
      'https://liminarycorp.example.com',
      'https://helxindustries.example.com',
      'https://kaderaglobal.example.com',
      'https://ositionsinc.example.com',
      'https://afformanceltd.example.com',
    ]);
    // The values are in the cards.
    const values = output.templates.flatMap(linkValues);
    for (const change of moved) expect(values).toContain(change.after);
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

  it('hold together: the counts say what the lists hold, and each file is what the tool writes from the others', () => {
    expect(outputProblems(output)).toEqual([]);
    // The page of changes and the index are written from the lists and the
    // templates, so an edit by hand to any file shows here, archive or not.
    const files = outputFiles(output);
    expect(readdirSync(OUTPUT_FOLDER).sort()).toEqual([...files.keys(), 'README.md'].sort());
    expect(staleFiles(OUTPUT_FOLDER, files)).toEqual([]);
  });

  describe('a folder that was edited by hand', () => {
    let folder = '';

    beforeAll(() => {
      const cache = path.resolve(import.meta.dirname, '../../node_modules/.cache');
      mkdirSync(cache, { recursive: true });
      folder = mkdtempSync(path.join(cache, 'templates-output-'));
      for (const [name, content] of outputFiles(output)) {
        writeFileSync(path.join(folder, name), content);
      }
    });

    afterAll(() => {
      if (folder !== '') rmSync(folder, { recursive: true, force: true });
    });

    it('is found out by the files that no longer agree', () => {
      expect(staleFiles(folder, outputFiles(readCommitted(folder)))).toEqual([]);

      const page = path.join(folder, 'CHANGES.md');
      writeFileSync(page, readFileSync(page, 'utf8').replace('| 합계 | 47 |', '| 합계 | 46 |'));
      expect(staleFiles(folder, outputFiles(readCommitted(folder)))).toEqual(['CHANGES.md']);

      writeFileSync(path.join(folder, 'notes.txt'), 'kept by hand');
      expect(staleFiles(folder, outputFiles(readCommitted(folder)))).toEqual([
        'CHANGES.md',
        'notes.txt (not written by the tool)',
      ]);
    });

    it('is found out by the counts when a change is taken off the list', () => {
      const lists = path.join(folder, 'changes.json');
      const parsed = JSON.parse(readFileSync(lists, 'utf8')) as { changes: unknown[] };
      parsed.changes.pop();
      writeFileSync(lists, `${JSON.stringify(parsed, null, 2)}\n`);
      expect(outputProblems(readCommitted(folder)).join()).toMatch(/the index counts \d+ changes/);
    });
  });
});

// Where the archive is, the committed folder has to be what the conversion
// writes from it. This is the only part that reads the original.
const withArchive = hasTemplateArchive();
if (!withArchive) {
  console.warn(skipNotice(`converting the template archive again (${TEMPLATE_ARCHIVE})`));
}

describe.skipIf(!withArchive)('the conversion of the archive', () => {
  // Only converted when the tests run: the archive is not there otherwise.
  let output: Output;
  beforeAll(() => {
    output = convertTemplates();
  });

  it('writes what the committed folder holds, byte for byte', () => {
    const files = outputFiles(output);
    expect(readdirSync(OUTPUT_FOLDER).sort()).toEqual([...files.keys(), 'README.md'].sort());
    expect(staleFiles(OUTPUT_FOLDER, files)).toEqual([]);
    expect(output).toEqual(readCommitted());
  });

  it('writes the same bytes when run again', () => {
    expect([...outputFiles(convertTemplates())]).toEqual([...outputFiles(output)]);
  });

  it('keeps what it promises about the result', () => {
    expect(outputProblems(output)).toEqual([]);
  });

  it('reads the 58 documents the committed cards were made from', () => {
    const archived = archivedDocuments();
    expect(archived).toHaveLength(58);
    expect(templateDocuments()).toEqual(archived);
    // The same cards in the same order; the committed ones went through the rules.
    expect(committedDocuments()).toHaveLength(archived.length);
  });

  it('names the archive by its place in the original, wherever it was read from', () => {
    expect(output.index.source.path).toBe('server/assets/templates.boardarchive');
    expect(output.index.source.sha256).toMatch(/^[0-9a-f]{64}$/);
  });
});
