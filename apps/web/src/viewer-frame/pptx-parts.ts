/**
 * The parts of a presentation sorted as the renderer takes them (its
 * `PptxFiles`, with the part names it reads: `parseZip` of
 * @aiden0z/pptx-renderer), in the PowerPoint worker. Each XML part is read
 * as text once, here, and the renderer gets that very text; each is measured
 * (`pptx-xml.ts`) for the worker's ceilings (`pptx-ceilings.ts`).
 *
 * Left out, so that the frame neither reads nor draws them: embedded fonts
 * (the app's fonts stand in), charts (a placeholder stands in), pictures of
 * kinds other than those `pptx-images.ts` reads, sounds and films, and
 * every relationship to something outside the file or to a web page (the
 * relationship parts are written anew without them). What a slide links to
 * within the presentation stays.
 */
import { SaxesParser } from 'saxes';

import type { PptxParts } from './pptx-protocol.ts';
import { pixelsOf, type Pixels } from './pptx-images.ts';
import { measure, type PartMeasure } from './pptx-xml.ts';
import { UnreadableFile } from './renderer.ts';

const RELATIONSHIPS = 'http://schemas.openxmlformats.org/package/2006/relationships';
const HYPERLINK = '/hyperlink';

/** The maps of `PptxParts` the parts are sorted into, by the names the renderer reads. */
const SORTED = [
  [/^ppt\/slides\/_rels\/[^/]+\.xml\.rels$/, 'slideRels'],
  [/^ppt\/slides\/[^/]+\.xml$/, 'slides'],
  [/^ppt\/slideLayouts\/_rels\/[^/]+\.xml\.rels$/, 'slideLayoutRels'],
  [/^ppt\/slideLayouts\/[^/]+\.xml$/, 'slideLayouts'],
  [/^ppt\/slideMasters\/_rels\/[^/]+\.xml\.rels$/, 'slideMasterRels'],
  [/^ppt\/slideMasters\/[^/]+\.xml$/, 'slideMasters'],
  [/^ppt\/theme\/(?!themeOverride[^/]*\.xml$)[^/]+\.xml$/, 'themes'],
  [/^ppt\/diagrams\/[^/]+\.xml$/, 'diagramDrawings'],
] as const;

type SortedMap = (typeof SORTED)[number][1];

const SINGLE = {
  '[Content_Types].xml': 'contentTypes',
  'ppt/presentation.xml': 'presentation',
  'ppt/_rels/presentation.xml.rels': 'presentationRels',
  'ppt/tableStyles.xml': 'tableStyles',
} as const;

/** One relationship of a part, as its relationship part says it. */
export interface Relationship {
  id: string;
  type: string;
  target: string;
}

/** What the worker knows of the presentation once the parts are sorted. */
export interface Sorted {
  parts: PptxParts;
  /** Each XML part kept, measured, by its name. */
  measures: Map<string, PartMeasure>;
  /** Each relationship part kept, read, by its name. */
  relationships: Map<string, Relationship[]>;
  /** Each picture kept, by its name under `ppt/media/`. */
  pixels: Map<string, Pixels>;
}

const decoder = new TextDecoder('utf-8');

/** The relationships of a relationship part, those to outside the file and to web pages left out. */
function readRelationships(text: string): Relationship[] {
  const kept: Relationship[] = [];
  const parser = new SaxesParser({ xmlns: true, position: false });
  parser.on('doctype', () => {
    throw new UnreadableFile();
  });
  parser.on('opentag', (tag) => {
    if (tag.local !== 'Relationship' || tag.uri !== RELATIONSHIPS) return;
    const value = (name: string): string => tag.attributes[name]?.value ?? '';
    const relationship = { id: value('Id'), type: value('Type'), target: value('Target') };
    const outside = value('TargetMode').trim().toLowerCase() === 'external';
    if (!outside && !relationship.type.endsWith(HYPERLINK)) kept.push(relationship);
  });
  parser.on('error', () => {
    throw new UnreadableFile();
  });
  parser.write(text).close();
  return kept;
}

const escape = (value: string): string =>
  value.replace(
    /[&<>"]/g,
    (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[character] ?? '',
  );

/** A relationship part of only these relationships. */
export function writeRelationships(relationships: readonly Relationship[]): string {
  const each = relationships.map(
    ({ id, type, target }) =>
      `<Relationship Id="${escape(id)}" Type="${escape(type)}" Target="${escape(target)}"/>`,
  );
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="${RELATIONSHIPS}">${each.join('')}</Relationships>`;
}

/** The name the renderer also finds a part by: each segment of it URI-decoded. */
function decodedName(name: string): string {
  return name
    .split('/')
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    })
    .join('/');
}

/** Sets a part under its name and, as the renderer does, under its decoded name when no part has it. */
function put<T>(map: Map<string, T>, name: string, value: T): void {
  map.set(name, value);
  const decoded = decodedName(name);
  if (decoded !== name && !map.has(decoded)) map.set(decoded, value);
}

function emptyParts(): PptxParts {
  const map = (): Map<string, string> => new Map();
  return {
    contentTypes: '',
    presentation: '',
    presentationRels: '',
    slides: map(),
    slideRels: map(),
    slideLayouts: map(),
    slideLayoutRels: map(),
    slideMasters: map(),
    slideMasterRels: map(),
    themes: map(),
    media: new Map(),
    charts: map(),
    chartStyles: map(),
    chartColors: map(),
    diagramDrawings: map(),
  };
}

const isRelationshipPart = (name: string): boolean => name.endsWith('.rels');

/** The text of an XML part, measured and, for a relationship part, written anew. */
function readPart(name: string, bytes: Uint8Array, sorted: Sorted): string {
  let text = decoder.decode(bytes);
  if (isRelationshipPart(name)) {
    const relationships = readRelationships(text);
    sorted.relationships.set(name, relationships);
    text = writeRelationships(relationships);
  }
  sorted.measures.set(name, measure(text));
  return text;
}

/** Sorts the unpacked parts; throws `UnreadableFile` when an XML part kept is not one the frame reads alike. */
export function sortParts(unpacked: ReadonlyMap<string, Uint8Array>): Sorted {
  const sorted: Sorted = {
    parts: emptyParts(),
    measures: new Map(),
    relationships: new Map(),
    pixels: new Map(),
  };
  const { parts } = sorted;
  for (const [name, bytes] of unpacked) {
    if (name.startsWith('ppt/media/')) {
      const pixels = pixelsOf(bytes);
      if (!pixels) continue;
      put(parts.media, name, bytes);
      sorted.pixels.set(name, pixels);
      continue;
    }
    const single = Object.hasOwn(SINGLE, name) ? SINGLE[name as keyof typeof SINGLE] : undefined;
    if (single) {
      parts[single] = readPart(name, bytes, sorted);
      continue;
    }
    const map: SortedMap | undefined = SORTED.find(([pattern]) => pattern.test(name))?.[1];
    if (map) put(parts[map], name, readPart(name, bytes, sorted));
  }
  return sorted;
}
