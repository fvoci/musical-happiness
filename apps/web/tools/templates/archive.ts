/**
 * Reads the built-in templates of the original and turns the content of each
 * card into one document body.
 *
 * The archive (server/assets/templates.boardarchive, read only) holds each
 * card's content as separate blocks: text blocks written in Markdown, image,
 * checkbox and divider blocks, in the order of the card's `contentOrder`.
 * Nothing here changes what the texts say; convert.ts does that, and counts.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { marked } from 'marked';

import { addNotes, markdownToBlocks, type ConversionNotes } from '../../src/lib/editor/markdown.ts';
import type { DocNode } from '../../src/lib/editor/schema.ts';
import { listZipMembers, readZipMember } from '../zip.ts';

export const ARCHIVE = path.resolve(
  import.meta.dirname,
  '../../../../server/assets/templates.boardarchive',
);

/** A block as the archive has it. */
export interface ArchivedBlock {
  id: string;
  parentId: string;
  type: string;
  title: string;
  fields: Record<string, unknown>;
}

/**
 * A board as the archive has it. Older boards are blocks of type `board` with
 * their members under `fields`; newer ones are a line of their own kind with
 * the members at the top. Both are read into this one shape.
 */
export interface ArchivedBoard {
  id: string;
  title: string;
  description: string;
  icon: string;
  showDescription: boolean;
  isTemplate: boolean;
  cardProperties: unknown[];
  properties: Record<string, unknown>;
}

export interface ArchivedTemplate {
  /** The folder of the board inside the archive; its files lie next to it. */
  folder: string;
  board: ArchivedBoard;
  /** Every block of the board, the board itself excluded, in the order of the archive. */
  blocks: ArchivedBlock[];
  /** The files in the board's folder: name and size in bytes. */
  files: { name: string; bytes: number }[];
}

const text = (value: unknown): string => (typeof value === 'string' ? value : '');

function readBoardLine(type: string, data: Record<string, unknown>): ArchivedBoard {
  // The members of an older board lie under `fields`.
  const source = type === 'board' ? data : ((data.fields ?? {}) as Record<string, unknown>);
  return {
    id: text(data.id),
    title: text(data.title),
    description: text(source.description),
    icon: text(source.icon),
    showDescription: source.showDescription === true,
    isTemplate: source.isTemplate === true,
    cardProperties: Array.isArray(source.cardProperties)
      ? (source.cardProperties as unknown[])
      : [],
    properties:
      typeof source.properties === 'object' && source.properties !== null
        ? (source.properties as Record<string, unknown>)
        : {},
  };
}

/** The templates of the archive, in the order of the archive. */
export function readArchive(file: string = ARCHIVE): ArchivedTemplate[] {
  const archive = readFileSync(file);
  const members = listZipMembers(archive);
  const templates: ArchivedTemplate[] = [];
  for (const member of members.filter((name) => name.endsWith('/board.jsonl'))) {
    const folder = member.slice(0, -'/board.jsonl'.length);
    let board: ArchivedBoard | undefined;
    const blocks: ArchivedBlock[] = [];
    for (const line of readZipMember(archive, member).toString('utf8').split('\n')) {
      if (line.trim() === '') continue;
      const entry = JSON.parse(line) as { type: string; data: Record<string, unknown> };
      if (entry.type === 'board' || (entry.type === 'block' && entry.data.type === 'board')) {
        board = readBoardLine(entry.type, entry.data);
      } else if (entry.type === 'block') {
        blocks.push(entry.data as unknown as ArchivedBlock);
      }
    }
    if (!board) throw new Error(`${member}: no board in it`);
    const files = members
      .filter((name) => name.startsWith(`${folder}/`) && name !== member)
      .map((name) => ({
        name: name.slice(folder.length + 1),
        bytes: readZipMember(archive, name).length,
      }));
    templates.push({ folder, board, blocks, files });
  }
  return templates;
}

export const CONTENT_TYPES = [
  'text',
  'checkbox',
  'image',
  'divider',
  'h1',
  'h2',
  'h3',
  'attachment',
];

export const textOf = (node: DocNode): string =>
  (node.text ?? '') + (node.content ?? []).map(textOf).join(' ');

/**
 * The letters of a text, for comparing what two renderings say: no spaces, no
 * markup. `plainOfMarkdown` is what the original's renderer would show.
 */
const letters = (value: string): string => value.replace(/\s+/g, '');
function plainOfMarkdown(markdown: string): string {
  const html = marked.parse(markdown, { gfm: true, breaks: true, async: false });
  const shown = html
    .replace(/<[^>]*>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
  return letters(shown);
}

/** What became of the content of one card. */
export interface CardBody {
  /** The document; undefined when the card has no content to show. */
  doc: DocNode | undefined;
  /** The blocks that went into the document, counted by type. */
  blocks: Record<string, number>;
  /** What the conversion could not carry over, by kind. */
  notes: ConversionNotes;
  /** The bytes of the titles of the blocks: the size of the source. */
  sourceBytes: number;
  /** Identifiers in the content order that name no block. */
  missingBlocks: number;
  /** Blocks of the card that are not in its content order: the original never shows them. */
  blocksOutsideOrder: number;
  /** Rows of blocks placed side by side; a document has no columns, so they follow each other. */
  sideBySideRows: number;
  /** Text blocks whose document does not say, letter for letter, what the Markdown renderer shows. */
  textMismatches: number;
}

/** The content blocks of a card, in the order of its `contentOrder`, as one document. */
export function cardBody(card: ArchivedBlock, blocks: readonly ArchivedBlock[]): CardBody {
  const byId = new Map(blocks.map((block) => [block.id, block]));
  const order = (card.fields.contentOrder as (string | string[])[] | undefined) ?? [];
  const ordered = order.flat();
  const children = blocks.filter((block) => block.parentId === card.id && block.type !== 'comment');
  const body: CardBody = {
    doc: undefined,
    blocks: {},
    notes: {},
    sourceBytes: 0,
    missingBlocks: 0,
    blocksOutsideOrder: children.filter((block) => !ordered.includes(block.id)).length,
    sideBySideRows: order.filter((entry) => Array.isArray(entry)).length,
    textMismatches: 0,
  };

  const content: DocNode[] = [];
  for (const id of ordered) {
    const block = byId.get(id);
    if (!block) {
      body.missingBlocks += 1;
      continue;
    }
    body.blocks[block.type] = (body.blocks[block.type] ?? 0) + 1;
    body.sourceBytes += Buffer.byteLength(block.title);
    switch (block.type) {
      case 'text': {
        const converted = markdownToBlocks(block.title);
        const said = letters(converted.content.map(textOf).join(''));
        if (said !== plainOfMarkdown(block.title)) body.textMismatches += 1;
        content.push(...converted.content);
        addNotes(body.notes, converted.notes);
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
        body.notes[`block-${block.type}`] = (body.notes[`block-${block.type}`] ?? 0) + 1;
    }
  }
  if (content.length > 0) body.doc = { type: 'doc', content };
  return body;
}

/**
 * Every document the templates hold: one per card that has content. For
 * tests that want real documents to work on.
 */
export function templateDocuments(
  file: string = ARCHIVE,
): { template: string; card: string; doc: DocNode }[] {
  const documents: { template: string; card: string; doc: DocNode }[] = [];
  for (const template of readArchive(file)) {
    for (const card of template.blocks.filter((block) => block.type === 'card')) {
      const { doc } = cardBody(card, template.blocks);
      if (doc) documents.push({ template: template.board.title, card: card.title, doc });
    }
  }
  return documents;
}
