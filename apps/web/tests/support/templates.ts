/**
 * The built-in templates, read from the one place they live: the server's
 * `crates/server/assets/templates/`. The server embeds those files in its
 * binary, and the in-memory backend reads them too (state/memory/seed.ts).
 * Each `NN-<key>.json` is one template; its texts are edited there by hand.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import type { DocNode } from '$lib/editor/schema.ts';
import type { SeedTemplate } from '$lib/state/memory/seed.ts';

export const TEMPLATES_FOLDER = path.resolve(
  import.meta.dirname,
  '../../../../crates/server/assets/templates',
);

/** The template files, in the order of their names (the order the server stores them in). */
export function readTemplates(folder: string = TEMPLATES_FOLDER): SeedTemplate[] {
  return readdirSync(folder)
    .filter((name) => /^\d+-.+\.json$/.test(name))
    .sort()
    .map((name) => JSON.parse(readFileSync(path.join(folder, name), 'utf8')) as SeedTemplate);
}

/** A card's body, with the titles of its template and its card. */
export interface TemplateDocument {
  template: string;
  card: string;
  doc: DocNode;
}

/** Every body of a card in the built-in templates. */
export function templateDocuments(): TemplateDocument[] {
  return readTemplates().flatMap((template) =>
    template.cards.flatMap((card) =>
      card.body ? [{ template: template.board.title, card: card.title, doc: card.body }] : [],
    ),
  );
}
