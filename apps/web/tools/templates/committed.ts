/**
 * Reads the output of the conversion back from its folder
 * (tools/templates/output): what stands in for the archive where the
 * original is not there. Nothing here reads the archive.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

import type { TemplateDocument } from './archive.ts';
import type { Output, Template } from './output.ts';
import type { Change } from './rewrite.ts';

export const OUTPUT_FOLDER = path.resolve(import.meta.dirname, 'output');

/** Written by hand; every other file of the folder is the tool's. */
export const HAND_WRITTEN = 'README.md';

function readJson(folder: string, name: string): unknown {
  return JSON.parse(readFileSync(path.join(folder, name), 'utf8')) as unknown;
}

/** The output a folder holds, in the shape the conversion returns it. */
export function readCommitted(folder: string = OUTPUT_FOLDER): Output {
  const index = readJson(folder, 'index.json') as Output['index'];
  const lists = readJson(folder, 'changes.json') as {
    changes: Change[];
    reviewNotes: Output['reviewNotes'];
  };
  return {
    index,
    templates: index.templates.map(({ file }) => readJson(folder, file) as Template),
    changes: lists.changes,
    reviewNotes: lists.reviewNotes,
  };
}

/**
 * The files of a folder that are not what `files` holds: missing, different,
 * or there without being written by the tool.
 */
export function staleFiles(folder: string, files: ReadonlyMap<string, string>): string[] {
  const stale = [...files]
    .filter(
      ([name, content]) =>
        !existsSync(path.join(folder, name)) ||
        readFileSync(path.join(folder, name), 'utf8') !== content,
    )
    .map(([name]) => name);
  const stray = existsSync(folder)
    ? readdirSync(folder).filter((name) => !files.has(name) && name !== HAND_WRITTEN)
    : [];
  return [...stale, ...stray.map((name) => `${name} (not written by the tool)`)];
}

/** Every document the committed templates hold: one per card that has a body. */
export function committedDocuments(folder: string = OUTPUT_FOLDER): TemplateDocument[] {
  const documents: TemplateDocument[] = [];
  for (const template of readCommitted(folder).templates) {
    for (const card of template.cards) {
      if (card.body) {
        documents.push({ template: template.board.title, card: card.title, doc: card.body });
      }
    }
  }
  return documents;
}
