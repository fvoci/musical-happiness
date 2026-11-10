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
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { addNotes, type ConversionNotes } from '../../src/lib/editor/markdown.ts';
import { storedSize, validateDoc, type DocNode } from '../../src/lib/editor/schema.ts';

import { CONTENT_TYPES, cardBody, readArchive, textOf } from './archive.ts';

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

const templates = readArchive();

const totals = {
  templates: templates.length,
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

for (const { board, blocks } of templates) {
  const { title } = board;
  const byId = new Map(blocks.map((block) => [block.id, block]));
  const template = { cards: 0, blocks: 0, notes: {} as ConversionNotes, bytes: 0 };

  totals.blocksWithoutCard += blocks.filter(
    (block) => CONTENT_TYPES.includes(block.type) && byId.get(block.parentId)?.type !== 'card',
  ).length;

  for (const card of blocks.filter((block) => block.type === 'card')) {
    totals.cards += 1;
    template.cards += 1;
    const body = cardBody(card, blocks);
    totals.sideBySideRows += body.sideBySideRows;
    totals.blocksOutsideOrder += body.blocksOutsideOrder;
    totals.missingBlocks += body.missingBlocks;
    totals.textMismatches += body.textMismatches;
    for (const [type, count] of Object.entries(body.blocks)) {
      totals.blocks[type] = (totals.blocks[type] ?? 0) + count;
      template.blocks += count;
    }
    const { doc } = body;
    if (!doc) continue;

    totals.cardsWithContent += 1;
    const problems = validateDoc(doc);
    if (problems.length > 0) {
      totals.invalid += 1;
      console.error(`${title} / ${card.title}: ${problems.join('; ')}`);
    }
    addNotes(totals.notes, body.notes);
    addNotes(template.notes, body.notes);
    countLinks(doc, hosts);
    totals.oldNames += (textOf(doc).match(OLD_NAMES) ?? []).length;
    const bytes = storedSize(doc);
    template.bytes += bytes;
    results.push({
      template: title,
      card: card.title,
      doc,
      bytes,
      sourceBytes: body.sourceBytes,
      ...measure(doc),
    });
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
