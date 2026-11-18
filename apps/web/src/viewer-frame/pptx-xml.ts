/**
 * What one XML part of a presentation would ask of the frame that draws
 * it, read in the PowerPoint worker (`pptx-worker.ts`) with saxes: how many
 * elements, how deep, how much text and how many tabs, how many shapes and
 * how deeply grouped, the cells of its tables, the pictures it shows and
 * which of them it recolours pixel by pixel, the relationships it names,
 * and its slide size if it has one.
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

/** A picture a part shows: the relationship that names it, and whether it is recoloured. */
export interface Shown {
  embed: string;
  recoloured: boolean;
}

export interface PartMeasure {
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
  /** The cells of the tables: rows times columns, summed. */
  cells: number;
  pictures: Shown[];
  /** The largest place or extent a shape or group gives (`off`, `ext`, `chOff`, `chExt`), in EMU, either way. */
  extent: number;
  /** The slides the part lists (`p:sldId`), when it is the presentation. */
  slideIds: number;
  /** The slide size (`p:sldSz`) in EMU, when the part gives one. */
  size?: { cx: number; cy: number };
  /** The characters of the text, each once. */
  characters: Set<string>;
}

interface Open {
  local: string;
  /** The picture this element is, when it is a `blip`. */
  shown?: Shown;
  /** The rows and columns of the table this element is, when it is a `tbl`. */
  table?: { rows: number; columns: number };
}

function emu(value: string | undefined): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : 0;
}

/** Measures one part; throws `UnreadableFile` when it is no XML the frame would read alike. */
export function measure(text: string): PartMeasure {
  const measured: PartMeasure = {
    elements: 0,
    depth: 0,
    text: 0,
    tabs: 0,
    shapes: 0,
    textBodies: 0,
    groups: 0,
    cells: 0,
    extent: 0,
    slideIds: 0,
    pictures: [],
    characters: new Set(),
  };
  const parser = new SaxesParser({ xmlns: true, position: false });
  const open: Open[] = [];
  let groups = 0;
  parser.on('doctype', () => {
    throw new UnreadableFile();
  });
  parser.on('opentag', (tag) => {
    const { local } = tag;
    const element: Open = { local };
    measured.elements += 1;
    measured.depth = Math.max(measured.depth, open.length + 1);
    if (SHAPES.has(local)) measured.shapes += 1;
    if (local === 'grpSp') {
      groups += 1;
      measured.groups = Math.max(measured.groups, groups);
    }
    const attributes = Object.values(tag.attributes);
    const value = (name: string): string | undefined =>
      attributes.find((each) => each.local === name)?.value;
    if (local === 'blip') {
      const embed = value('embed');
      if (embed !== undefined) {
        element.shown = { embed, recoloured: false };
        measured.pictures.push(element.shown);
      }
    } else if (PIXEL_EFFECTS.has(local)) {
      const blip = open.findLast((each) => each.local === 'blip');
      if (blip?.shown) blip.shown.recoloured = true;
    } else if (local === 'txBody' && !open.some((each) => each.local === 'tc')) {
      measured.textBodies += 1;
    } else if (local === 'tbl') {
      element.table = { rows: 0, columns: 0 };
    } else if (local === 'tr' || local === 'gridCol') {
      const table = open.findLast((each) => each.table)?.table;
      if (table && local === 'tr') table.rows += 1;
      else if (table) table.columns += 1;
    } else if (PLACES.has(local)) {
      for (const name of ['x', 'y', 'cx', 'cy']) {
        measured.extent = Math.max(measured.extent, Math.abs(emu(value(name))));
      }
    } else if (local === 'sldId') {
      measured.slideIds += 1;
    } else if (local === 'sldSz') {
      measured.size = { cx: emu(value('cx')), cy: emu(value('cy')) };
    }
    open.push(element);
  });
  parser.on('closetag', () => {
    const element = open.pop();
    if (element?.local === 'grpSp') groups -= 1;
    if (element?.table) measured.cells += element.table.rows * element.table.columns;
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
