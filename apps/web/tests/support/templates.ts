/**
 * The built-in templates, read from the one place they live: the server's
 * `crates/server/assets/templates/`, which embeds those files in its binary.
 * Each `NN-<key>.<language>.json` is one template in one language; the
 * pictures and documents its bodies show are in `files/`.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import type { DocNode } from '$lib/editor/schema.ts';
import type { SeedTemplate } from '$lib/state/memory/seed.ts';

export const TEMPLATES_FOLDER = path.resolve(
  import.meta.dirname,
  '../../../../crates/server/assets/templates',
);

/** A template file: a board as the in-memory backend's files write one, in a language, with its files. */
export type BuiltinTemplate = Omit<SeedTemplate, 'files'> & {
  /** The tag of the language it is written in. */
  language: string;
  /** The files its bodies show: the name in `files/`, and the name each is shown under. */
  files: { asset: string; name: string }[];
};

/** The template files, in the order of their names (the order the server stores them in). */
export function readTemplates(folder: string = TEMPLATES_FOLDER): BuiltinTemplate[] {
  return readdirSync(folder)
    .filter((name) => /^\d+-.+\.json$/.test(name))
    .sort()
    .map((name) => JSON.parse(readFileSync(path.join(folder, name), 'utf8')) as BuiltinTemplate);
}

/** The names of the files of `files/`. */
export function templateAssets(folder: string = TEMPLATES_FOLDER): string[] {
  return readdirSync(path.join(folder, 'files')).sort();
}

/** A card's body, with the titles of its template and its card. */
export interface TemplateDocument {
  template: string;
  card: string;
  doc: DocNode;
}

/** Every body of a card in the built-in templates, in every language. */
export function templateDocuments(): TemplateDocument[] {
  return readTemplates().flatMap((template) =>
    template.cards.flatMap((card) =>
      card.body ? [{ template: template.board.title, card: card.title, doc: card.body }] : [],
    ),
  );
}
