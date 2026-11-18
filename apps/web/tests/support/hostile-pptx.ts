/**
 * Small PowerPoint decks that would make the frame do a great deal: each
 * passes exactly one of the PowerPoint worker's ceilings
 * (src/viewer-frame/pptx-ceilings.ts), and no other, so that the tests show
 * each ceiling catches its case. Pictures that are to be large are a PNG
 * header alone: the worker reads the size a picture decodes to from its
 * header, as the browser does, and never decodes it.
 */
import { crc32 } from 'node:zlib';

import {
  EAGER_ELEMENT_LIMIT,
  EXTENT_LIMIT,
  PICTURE_PIXEL_LIMIT,
  SLIDE_LIMIT,
  SLIDE_LIMITS,
  SLIDE_SIDE_LIMIT,
} from '../../src/viewer-frame/pptx-ceilings.ts';

import { png } from './viewer-files.ts';
import {
  deck,
  group,
  NAMESPACES,
  picture,
  run,
  shape,
  SLIDE,
  table,
  textBox,
  type Link,
} from './viewer-pptx.ts';

const PLACE: [number, number, number, number] = [600_000, 600_000, 4_000_000, 2_000_000];

/** A PNG of its header alone: as large as it says, to whatever reads only the header. */
export function pngHeader(width: number, height: number): Buffer {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  const body = Buffer.concat([Buffer.from('IHDR', 'latin1'), header]);
  const length = Buffer.alloc(4);
  length.writeUInt32BE(13);
  const sum = Buffer.alloc(4);
  sum.writeUInt32BE(crc32(body));
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    length,
    body,
    sum,
  ]);
}

const one = (
  shapes: string,
  links: Link[] = [],
  parts: { name: string; data: Buffer }[] = [],
): Buffer => deck({ slides: [{ shapes, links }], parts });

const image = (name: string): Link => ({
  id: 'rIdPicture',
  type: 'image',
  target: `../media/${name}`,
});

/** Elements nested `depth` deep, inside a shape's extension list (which the renderer passes over). */
const nested = (depth: number): string =>
  `<a:extLst><a:ext uri="{fvoci}">${'<a:x>'.repeat(depth)}${'</a:x>'.repeat(depth)}</a:ext></a:extLst>`;

/** Every hostile deck, by the ceiling it passes (the name `TooMuch` is thrown with). */
export const HOSTILE_PPTX: Record<string, () => Buffer> = {
  slides: () => deck({ slides: Array.from({ length: SLIDE_LIMIT + 1 }, () => ({ shapes: '' })) }),
  elements: () =>
    one(
      textBox(
        2,
        PLACE,
        Array.from({ length: SLIDE_LIMITS.elements }, () => ''),
      ),
    ),
  depth: () =>
    one(
      shape(2, 'rect', PLACE, '2E7DD2').replace(
        '</p:spPr>',
        `${nested(SLIDE_LIMITS.depth)}</p:spPr>`,
      ),
    ),
  text: () => one(textBox(2, PLACE, [run('가'.repeat(SLIDE_LIMITS.text + 1))])),
  tabs: () => one(textBox(2, PLACE, [run('\t'.repeat(SLIDE_LIMITS.tabs + 1))])),
  shapes: () =>
    one(
      Array.from({ length: SLIDE_LIMITS.shapes + 1 }, (_, index) =>
        shape(index + 2, 'rect', [index * 1000, 0, 1000, 1000], '2E7DD2'),
      ).join(''),
    ),
  textBodies: () =>
    one(
      Array.from({ length: SLIDE_LIMITS.textBodies + 1 }, (_, index) =>
        textBox(index + 2, [index * 1000, 0, 1000, 1000], [run('가')]),
      ).join(''),
    ),
  work: () => {
    // Seven tenths of three ceilings: under each, past them together.
    const share = (limit: number): number => Math.ceil(limit * 0.7);
    const texts = Array.from({ length: share(SLIDE_LIMITS.textBodies) }, (_, index) =>
      textBox(index + 2, [index * 1000, 0, 1000, 1000], [run('가')]),
    );
    const shapes = Array.from({ length: share(SLIDE_LIMITS.shapes) - texts.length }, (_, index) =>
      shape(index + 1000, 'rect', [index * 1000, 0, 1000, 1000], '2E7DD2'),
    );
    const paragraphs = textBox(
      5000,
      PLACE,
      Array.from({ length: share(SLIDE_LIMITS.elements) }, () => ''),
    );
    return one([...texts, ...shapes, paragraphs].join(''));
  },
  groups: () => {
    let inside = shape(100, 'rect', PLACE, '2E7DD2');
    for (let level = 0; level <= SLIDE_LIMITS.groups; level += 1)
      inside = group(level + 2, PLACE, inside);
    return one(inside);
  },
  cells: () => {
    const columns = 50;
    const rows = Math.floor(SLIDE_LIMITS.cells / columns) + 1;
    return one(
      table(
        2,
        PLACE,
        Array.from({ length: rows }, () => Array.from({ length: columns }, () => '')),
      ),
    );
  },
  pictures: () =>
    one(
      Array.from({ length: SLIDE_LIMITS.pictures + 1 }, (_, index) =>
        picture(index + 2, 'rIdPicture', PLACE),
      ).join(''),
      [image('image1.png')],
      [{ name: 'media/image1.png', data: png(4, 4, [46, 125, 210]) }],
    ),
  pixels: () => {
    // Pictures each under the ceiling of one, together past the slide's.
    const side = Math.floor(Math.sqrt(PICTURE_PIXEL_LIMIT));
    const count = Math.floor(SLIDE_LIMITS.pixels / (side * side)) + 1;
    return one(
      Array.from({ length: count }, (_, index) => picture(index + 2, 'rIdPicture', PLACE)).join(''),
      [image('image1.png')],
      [{ name: 'media/image1.png', data: pngHeader(side, side) }],
    );
  },
  picture: () => {
    const side = Math.ceil(Math.sqrt(PICTURE_PIXEL_LIMIT)) + 1;
    // Not shown anywhere: a picture that large is refused wherever it is.
    return one('', [], [{ name: 'media/image1.png', data: pngHeader(side, side) }]);
  },
  recoloured: () => {
    const side = Math.ceil(Math.sqrt(SLIDE_LIMITS.recoloured)) + 1;
    return one(
      picture(2, 'rIdPicture', PLACE, '<a:lum bright="10000"/>'),
      [image('image1.png')],
      [{ name: 'media/image1.png', data: pngHeader(side, side) }],
    );
  },
  extent: () => one(shape(2, 'rect', [EXTENT_LIMIT + 1, 0, 1000, 1000], '2E7DD2')),
  'slide size': () =>
    deck({ slides: [{ shapes: '' }], size: { cx: SLIDE_SIDE_LIMIT + 1, cy: SLIDE.cy } }),
  'elements before the first slide': () =>
    deck({
      slides: [{ shapes: '' }],
      themeExtra: `<a:extLst><a:ext uri="{fvoci}">${'<a:x/>'.repeat(EAGER_ELEMENT_LIMIT)}</a:ext></a:extLst>`,
    }),
};

/**
 * A slide and its layout each under the ceiling of a slide's elements, past
 * it together: the layout's shapes are drawn on the slide.
 */
export function layoutAndSlidePptx(): Buffer {
  const half = Math.floor(SLIDE_LIMITS.elements / 2) + 10;
  const paragraphs = (count: number): string[] => Array.from({ length: count }, () => '');
  return deck({
    slides: [{ shapes: textBox(2, PLACE, paragraphs(half)) }],
    layoutShapes: textBox(2, PLACE, paragraphs(half)),
  });
}

/** A deck whose slide declares a document type, with an entity: no XML the frame reads as the worker does. */
export function doctypePptx(): Buffer {
  const xml = `<?xml version="1.0"?><!DOCTYPE p:sld [<!ENTITY big "가나다">]><p:sld ${NAMESPACES}><p:cSld><p:spTree/></p:cSld></p:sld>`;
  return deck({ slides: [{ shapes: '', xml }] });
}

/** A deck whose slide is not well formed. */
export function malformedPptx(): Buffer {
  return deck({ slides: [{ shapes: '', xml: `<p:sld ${NAMESPACES}><p:cSld>` }] });
}

/**
 * A deck of `slides` slides of `paragraphs` empty paragraphs each: under
 * every ceiling, but megabytes of XML that take the worker seconds to read.
 */
export function slowPptx(slides: number, paragraphs: number): Buffer {
  const shapes = textBox(
    2,
    PLACE,
    Array.from({ length: paragraphs }, () => ''),
  );
  return deck({ slides: Array.from({ length: slides }, () => ({ shapes })) });
}

/** A deck of `slides` slides of `boxes` text boxes each: each slide takes the frame a while to draw. */
export function heavyPptx(slides: number, boxes: number): Buffer {
  const shapes = Array.from({ length: boxes }, (_, index) =>
    textBox(
      index + 2,
      [(index % 20) * 600_000, Math.floor(index / 20) * 400_000, 600_000, 400_000],
      [run(`상자 ${index}`, 900)],
    ),
  ).join('');
  return deck({ slides: Array.from({ length: slides }, () => ({ shapes })) });
}
