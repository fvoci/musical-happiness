/**
 * Converts the built-in templates of the original into document bodies and
 * counts what the conversion changes.
 *
 *   node tools/templates/convert.ts [--write <directory>] [--json]
 *
 * The archive (server/assets/templates.boardarchive, read only) holds each
 * card's content as separate blocks: text blocks written in Markdown, image,
 * checkbox and divider blocks, in the order of the card's `contentOrder`.
 * Here every card becomes one document. The tool prints, per template and in
 * total: how many blocks of each kind went in, what could not be carried
 * over (by kind), and how large the documents are. `--write` also saves the
 * documents, for looking at; they are not part of the repository.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { marked } from 'marked';

import { addNotes, markdownToBlocks, type ConversionNotes } from '../../src/lib/editor/markdown.ts';
import { storedSize, validateDoc, type DocNode } from '../../src/lib/editor/schema.ts';
import { listZipMembers, readZipMember } from '../zip.ts';

const ARCHIVE = path.resolve(
  import.meta.dirname,
  '../../../../server/assets/templates.boardarchive',
);

/** A block as the archive has it. */
interface ArchivedBlock {
  id: string;
  parentId: string;
  type: string;
  title: string;
  fields: Record<string, unknown>;
}

interface CardResult {
  template: string;
  card: string;
  doc: DocNode;
  bytes: number;
  sourceBytes: number;
  nodes: number;
  depth: number;
}

/** The two names of the original product, put together from halves (see tools/names). */
const OLD_NAMES = new RegExp(
  [
    ['focal', 'board'],
    ['matter', 'most'],
  ]
    .map((halves) => halves.join(''))
    .join('|'),
  'gi',
);

function readBoard(archive: Buffer, member: string): { title: string; blocks: ArchivedBlock[] } {
  let title = '';
  const blocks: ArchivedBlock[] = [];
  for (const line of readZipMember(archive, member).toString('utf8').split('\n')) {
    if (line.trim() === '') continue;
    const entry = JSON.parse(line) as { type: string; data: ArchivedBlock };
    // A board is either its own kind of line or, in older boards, a block of type `board`.
    if (entry.type === 'board' || (entry.type === 'block' && entry.data.type === 'board')) {
      title = entry.data.title;
    } else if (entry.type === 'block') {
      blocks.push(entry.data);
    }
  }
  return { title, blocks };
}

function measure(node: DocNode, depth = 1): { nodes: number; depth: number } {
  let nodes = 1;
  let deepest = depth;
  for (const child of node.content ?? []) {
    const inner = measure(child, depth + 1);
    nodes += inner.nodes;
    deepest = Math.max(deepest, inner.depth);
  }
  return { nodes, depth: deepest };
}

function countLinks(node: DocNode, hosts: Map<string, number>): void {
  for (const mark of node.marks ?? []) {
    if (mark.type !== 'link') continue;
    const href = String(mark.attrs?.href);
    const host = /^[a-z]+:\/\/([^/?#]+)/i.exec(href)?.[1] ?? '(inside the app)';
    hosts.set(host, (hosts.get(host) ?? 0) + 1);
  }
  for (const child of node.content ?? []) countLinks(child, hosts);
}

const textOf = (node: DocNode): string =>
  (node.text ?? '') + (node.content ?? []).map(textOf).join(' ');

/**
 * The letters of a text, for comparing what two renderings say: no spaces, no
 * markup. `plainOfMarkdown` is what the original's renderer would show.
 */
const letters = (text: string): string => text.replace(/\s+/g, '');
function plainOfMarkdown(markdown: string): string {
  const html = marked.parse(markdown, { gfm: true, breaks: true, async: false });
  const text = html
    .replace(/<[^>]*>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
  return letters(text);
}

const archive = readFileSync(ARCHIVE);
const members = listZipMembers(archive).filter((name) => name.endsWith('/board.jsonl'));
const CONTENT_TYPES = ['text', 'checkbox', 'image', 'divider', 'h1', 'h2', 'h3', 'attachment'];

const totals = {
  templates: members.length,
  cards: 0,
  cardsWithContent: 0,
  blocks: {} as Record<string, number>,
  /** Blocks that belong to a card but are not in its content order: the original never shows them. */
  blocksOutsideOrder: 0,
  /** Content blocks whose card is not in the archive: left behind by deleted cards. */
  blocksWithoutCard: 0,
  /** Identifiers in a content order that name no block. */
  missingBlocks: 0,
  /** Rows of blocks placed side by side; a document has no columns, so they follow each other. */
  sideBySideRows: 0,
  notes: {} as ConversionNotes,
  invalid: 0,
  /** Text blocks whose document does not say, letter for letter, what the Markdown renderer shows. */
  textMismatches: 0,
  oldNames: 0,
};
const perTemplate: Record<
  string,
  { cards: number; blocks: number; notes: ConversionNotes; bytes: number }
> = {};
const results: CardResult[] = [];
const hosts = new Map<string, number>();

for (const member of members) {
  const { title, blocks } = readBoard(archive, member);
  const byId = new Map(blocks.map((block) => [block.id, block]));
  const template = { cards: 0, blocks: 0, notes: {} as ConversionNotes, bytes: 0 };

  totals.blocksWithoutCard += blocks.filter(
    (block) => CONTENT_TYPES.includes(block.type) && byId.get(block.parentId)?.type !== 'card',
  ).length;

  for (const card of blocks.filter((block) => block.type === 'card')) {
    totals.cards += 1;
    template.cards += 1;
    const order = (card.fields.contentOrder as (string | string[])[] | undefined) ?? [];
    totals.sideBySideRows += order.filter((entry) => Array.isArray(entry)).length;
    const ordered = order.flat();
    const children = blocks.filter(
      (block) => block.parentId === card.id && block.type !== 'comment',
    );
    totals.blocksOutsideOrder += children.filter((block) => !ordered.includes(block.id)).length;

    const content: DocNode[] = [];
    const notes: ConversionNotes = {};
    let sourceBytes = 0;
    for (const id of ordered) {
      const block = byId.get(id);
      if (!block) {
        totals.missingBlocks += 1;
        continue;
      }
      totals.blocks[block.type] = (totals.blocks[block.type] ?? 0) + 1;
      template.blocks += 1;
      sourceBytes += Buffer.byteLength(block.title);
      switch (block.type) {
        case 'text': {
          const converted = markdownToBlocks(block.title);
          const said = letters(converted.content.map(textOf).join(''));
          if (said !== plainOfMarkdown(block.title)) totals.textMismatches += 1;
          content.push(...converted.content);
          addNotes(notes, converted.notes);
          break;
        }
        case 'checkbox': {
          // Neighbouring checkbox blocks are one task list. Their text is plain, not Markdown.
          const item: DocNode = {
            type: 'taskItem',
            attrs: { checked: Boolean(block.fields.value) },
            content: [
              block.title === ''
                ? { type: 'paragraph' }
                : { type: 'paragraph', content: [{ type: 'text', text: block.title }] },
            ],
          };
          const last = content.at(-1);
          if (last?.type === 'taskList') last.content?.push(item);
          else content.push({ type: 'taskList', content: [item] });
          break;
        }
        case 'image':
          content.push({ type: 'image', attrs: { fileId: String(block.fields.fileId) } });
          break;
        case 'divider':
          content.push({ type: 'horizontalRule' });
          break;
        default:
          notes[`block-${block.type}`] = (notes[`block-${block.type}`] ?? 0) + 1;
      }
    }
    if (content.length === 0) continue;

    totals.cardsWithContent += 1;
    const doc: DocNode = { type: 'doc', content };
    const problems = validateDoc(doc);
    if (problems.length > 0) {
      totals.invalid += 1;
      console.error(`${title} / ${card.title}: ${problems.join('; ')}`);
    }
    addNotes(totals.notes, notes);
    addNotes(template.notes, notes);
    countLinks(doc, hosts);
    totals.oldNames += (textOf(doc).match(OLD_NAMES) ?? []).length;
    const bytes = storedSize(doc);
    template.bytes += bytes;
    results.push({ template: title, card: card.title, doc, bytes, sourceBytes, ...measure(doc) });
  }
  perTemplate[title] = template;
}

const sorted = (values: number[]): number[] => [...values].sort((a, b) => a - b);
function distribution(values: number[]): Record<string, number> {
  const list = sorted(values);
  const at = (share: number): number =>
    list[Math.min(list.length - 1, Math.floor(share * list.length))] ?? 0;
  return {
    count: list.length,
    min: list[0] ?? 0,
    median: at(0.5),
    p90: at(0.9),
    p99: at(0.99),
    max: list.at(-1) ?? 0,
    total: list.reduce((sum, value) => sum + value, 0),
  };
}

const report = {
  totals,
  bytes: distribution(results.map((result) => result.bytes)),
  sourceBytes: distribution(results.map((result) => result.sourceBytes)),
  growth: distribution(
    results
      .filter((result) => result.sourceBytes > 0)
      .map((result) => Math.round((result.bytes / result.sourceBytes) * 100)),
  ),
  nodes: distribution(results.map((result) => result.nodes)),
  depth: distribution(results.map((result) => result.depth)),
  largest: sorted(results.map((result) => result.bytes))
    .slice(-3)
    .map((bytes) => {
      const result = results.find((candidate) => candidate.bytes === bytes);
      return {
        bytes,
        nodes: result?.nodes,
        depth: result?.depth,
        sourceBytes: result?.sourceBytes,
      };
    }),
  linkHosts: [...hosts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([host, count]) => [host.replace(OLD_NAMES, '<original>'), count]),
  perTemplate: Object.entries(perTemplate).map(([title, value], index) => ({
    // Titles carry the original product's name in places; number them instead.
    template: `${index + 1}. ${title.replace(OLD_NAMES, '<original>')}`,
    ...value,
  })),
};

const writeTo = process.argv.includes('--write')
  ? process.argv[process.argv.indexOf('--write') + 1]
  : undefined;
if (writeTo) {
  mkdirSync(writeTo, { recursive: true });
  results.forEach((result, index) => {
    writeFileSync(
      path.join(writeTo, `${String(index).padStart(3, '0')}.json`),
      JSON.stringify(result, null, 2),
    );
  });
}
console.log(JSON.stringify(report, null, 2));
if (totals.invalid > 0) process.exit(1);
