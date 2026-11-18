/**
 * What one XML part of a presentation would ask of the frame that draws
 * it, read in the PowerPoint worker (`pptx-worker.ts`) with saxes: how many
 * elements, how deep, how much text and how many tabs, how many shapes and
 * how deeply grouped, the cells of its tables, the pictures it shows and
 * which of them it recolours pixel by pixel, the SmartArt it draws, and
 * its slide size if it has one.
 *
 * What it cannot read as the renderer does, it counts more of: a picture is
 * any value of an attribute that names a picture's relationship, wherever
 * it is, and a table's cells are its `a:tc`, or its rows times its columns
 * when that is more.
 *
 * The frame parses the same text with the browser's `DOMParser`, so what is
 * read here is what is drawn: a part that is not well formed (with
 * namespaces, as the browser reads it) or declares a document type (whose
 * entities would make the browser's tree another than this one) is no part
 * the viewer reads.
 */
import { SaxesParser } from 'saxes';

import { UnreadableFile } from './renderer.ts';

/** The elements that are a shape on a slide, whatever their prefix. */
const SHAPES = new Set(['sp', 'pic', 'cxnSp', 'grpSp', 'graphicFrame', 'contentPart']);
/** The elements that place a shape or a group's children. */
const PLACES = new Set(['off', 'ext', 'chOff', 'chExt']);
/** The effects of a picture the renderer works out pixel by pixel, on the frame's thread. */
const PIXEL_EFFECTS = new Set(['duotone', 'lum', 'biLevel']);
/** The longest attribute value kept as one that may name a relationship. */
const NAME_LENGTH = 255;
/** The slide size the renderer takes for a side the presentation does not give (its default), in EMU. */
const DEFAULT_SIZE = { cx: 9_144_000, cy: 6_858_000 };

/** A picture fill (`a:blip`): the values of the attributes in it, and how many pixel effects it has. */
export interface Blip {
  names: Set<string>;
  effects: number;
}

export interface PartMeasure {
  /** The length of the part's text: what the browser parses. */
  bytes: number;
  elements: number;
  depth: number;
  /** Characters of text. */
  text: number;
  /** Tab characters in the text: the renderer lays the text out once for each. */
  tabs: number;
  shapes: number;
  /** The shapes' text bodies, those of table cells left out: the renderer fits each to its box by measuring it. */
  textBodies: number;
  /** How deeply groups are nested in groups (0: no group). */
  groups: number;
  /** The cells of the tables, summed. */
  cells: number;
  /** How often each attribute value occurs (those up to `NAME_LENGTH` long): a relationship is named by one. */
  names: Map<string, number>;
  blips: Blip[];
  /** The SmartArt the part shows (`a:graphicData` of a diagram): each draws a drawing of its own. */
  diagrams: number;
  /** The largest place or extent a shape or group gives (`off`, `ext`, `chOff`, `chExt`), in EMU, either way. */
  extent: number;
  /** The slides the part lists (`p:sldId`), when it is the presentation. */
  slideIds: number;
  /** The slide size (`p:sldSz` under the root) in EMU, as the renderer reads it, when the part gives one. */
  size?: { cx: number; cy: number };
  /** The characters of the text, each once. */
  characters: Set<string>;
}

interface Open {
  local: string;
  /** The picture fill this element is, when it is a `blip`. */
  blip?: Blip;
  /** The table this element is, when it is a `tbl`. */
  table?: { rows: number; columns: number; cells: number };
}

function emu(value: string | undefined): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : 0;
}

/** A side of the slide as the renderer reads it: the number, or its default when there is none. */
function side(value: string | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  const number = Number(value);
  return Number.isNaN(number) ? fallback : number;
}

function emptyMeasure(bytes: number): PartMeasure {
  return {
    bytes,
    elements: 0,
    depth: 0,
    text: 0,
    tabs: 0,
    shapes: 0,
    textBodies: 0,
    groups: 0,
    cells: 0,
    names: new Map(),
    blips: [],
    diagrams: 0,
    extent: 0,
    slideIds: 0,
    characters: new Set(),
  };
}

type Tag = { local: string; attributes: Record<string, { local: string; value: string }> };

/** What an element adds to the measure, besides the counts every element has. */
function take(tag: Tag, open: Open[], measured: PartMeasure, element: Open): void {
  const { local } = tag;
  const attributes = Object.values(tag.attributes);
  const value = (name: string): string | undefined =>
    attributes.find((each) => each.local === name)?.value;
  const blip = open.findLast((each) => each.blip)?.blip;
  for (const { value: named } of attributes) {
    if (named.length > NAME_LENGTH) continue;
    measured.names.set(named, (measured.names.get(named) ?? 0) + 1);
    blip?.names.add(named);
  }
  if (local === 'blip') {
    element.blip = { names: new Set(attributes.map((each) => each.value)), effects: 0 };
    measured.blips.push(element.blip);
  } else if (PIXEL_EFFECTS.has(local) && blip) {
    blip.effects += 1;
  } else if (local === 'txBody' && !open.some((each) => each.local === 'tc')) {
    measured.textBodies += 1;
  } else if (local === 'tbl') {
    element.table = { rows: 0, columns: 0, cells: 0 };
  } else if (local === 'tr' || local === 'gridCol' || local === 'tc') {
    const table = open.findLast((each) => each.table)?.table;
    if (table) table[local === 'tr' ? 'rows' : local === 'gridCol' ? 'columns' : 'cells'] += 1;
  } else if (local === 'graphicData' && (value('uri') ?? '').includes('diagram')) {
    measured.diagrams += 1;
  } else if (PLACES.has(local)) {
    for (const name of ['x', 'y', 'cx', 'cy']) {
      measured.extent = Math.max(measured.extent, Math.abs(emu(value(name))));
    }
  } else if (local === 'sldId') {
    measured.slideIds += 1;
  } else if (local === 'sldSz' && open.length === 1 && !measured.size) {
    // The renderer reads the first under the root (its `child`), and a side that is no number as its default.
    measured.size = {
      cx: side(value('cx'), DEFAULT_SIZE.cx),
      cy: side(value('cy'), DEFAULT_SIZE.cy),
    };
  }
}

/** Measures one part; throws `UnreadableFile` when it is no XML the frame would read alike. */
export function measure(text: string): PartMeasure {
  const measured = emptyMeasure(text.length);
  const parser = new SaxesParser({ xmlns: true, position: false });
  const open: Open[] = [];
  let groups = 0;
  parser.on('doctype', () => {
    throw new UnreadableFile();
  });
  parser.on('opentag', (tag) => {
    const element: Open = { local: tag.local };
    measured.elements += 1;
    measured.depth = Math.max(measured.depth, open.length + 1);
    if (SHAPES.has(tag.local)) measured.shapes += 1;
    if (tag.local === 'grpSp') {
      groups += 1;
      measured.groups = Math.max(measured.groups, groups);
    }
    take(tag, open, measured, element);
    open.push(element);
  });
  parser.on('closetag', () => {
    const element = open.pop();
    if (element?.local === 'grpSp') groups -= 1;
    const table = element?.table;
    if (table) measured.cells += Math.max(table.cells, table.rows * table.columns);
  });
  const addText = (data: string): void => {
    measured.text += data.length;
    for (const character of data) {
      if (character === '\t') measured.tabs += 1;
      measured.characters.add(character);
    }
  };
  parser.on('text', addText);
  parser.on('cdata', addText);
  parser.on('error', () => {
    throw new UnreadableFile();
  });
  parser.write(text).close();
  return measured;
}
