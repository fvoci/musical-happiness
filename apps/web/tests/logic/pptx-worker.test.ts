import { describe, expect, it } from 'vitest';

import { examine } from '$lib/file-viewer/formats.ts';
import { checkCeilings, resolve, SLIDE_LIMITS } from '../../src/viewer-frame/pptx-ceilings.ts';
import { pixelsOf } from '../../src/viewer-frame/pptx-images.ts';
import { sortParts, type Sorted } from '../../src/viewer-frame/pptx-parts.ts';
import { readFromPptxWorker } from '../../src/viewer-frame/pptx-protocol.ts';
import { readPresentation } from '../../src/viewer-frame/pptx-read.ts';
import { unpack } from '../../src/viewer-frame/pptx-unpack.ts';
import { TooMuch } from '../../src/viewer-frame/worker-xml.ts';
import { repack } from '../../src/viewer-frame/zip-guard.ts';

import {
  doctypePptx,
  HOSTILE_PPTX,
  layoutAndSlidePptx,
  malformedPptx,
  pngHeader,
} from '../support/hostile-pptx.ts';
import { png, zip } from '../support/viewer-files.ts';
import { deck, koreanPptx, picture, PPTX_LINK, PPTX_TEXT } from '../support/viewer-pptx.ts';

/** The archive as the frame hands it to the worker: counted and written anew. */
const repacked = (file: Buffer): Promise<ArrayBuffer> => repack(Uint8Array.from(file).buffer);

const sorted = async (file: Buffer): Promise<Sorted> =>
  sortParts(await unpack(await repacked(file)));

/** The ceiling a deck is stopped at, or undefined when it passes them all. */
async function stoppedAt(file: Buffer): Promise<string | undefined> {
  const parts = await sorted(file);
  try {
    checkCeilings(parts);
    return undefined;
  } catch (error) {
    if (error instanceof TooMuch) return error.message;
    throw error;
  }
}

describe('a presentation is told apart by its first bytes', () => {
  it('a ZIP archive with ppt/presentation.xml', () => {
    expect(examine(Uint8Array.from(koreanPptx()))).toEqual({ kind: 'format', format: 'pptx' });
  });

  it('an old PowerPoint file (a compound file) is not one the viewer draws', () => {
    const compound = Buffer.alloc(1024);
    compound.set([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    expect(examine(Uint8Array.from(compound))).toEqual({ kind: 'unknown' });
  });
});

describe('the PowerPoint worker reads a presentation into the parts the renderer takes', () => {
  it('the Korean sample: its slides, picture and text; no chart, no link outside', async () => {
    const answer = await readPresentation(await repacked(koreanPptx()));
    if (answer.kind !== 'read') throw new Error(JSON.stringify(answer));
    const { parts } = answer;
    expect([...parts.slides.keys()].sort()).toEqual([
      'ppt/slides/slide1.xml',
      'ppt/slides/slide2.xml',
      'ppt/slides/slide3.xml',
    ]);
    expect(parts.slides.get('ppt/slides/slide1.xml')).toContain(PPTX_TEXT.title);
    expect([...parts.media.keys()]).toEqual(['ppt/media/image1.png']);
    expect(parts.charts.size).toBe(0);
    // The jump to a slide stays; the link to a web page is gone.
    const second = parts.slideRels.get('ppt/slides/_rels/slide2.xml.rels') ?? '';
    expect(second).not.toContain(PPTX_LINK);
    expect(second).not.toContain('rIdLink');
    expect(parts.slideRels.get('ppt/slides/_rels/slide1.xml.rels')).toContain('rIdJump');
    for (const character of '분기 보고') expect(answer.characters).toContain(character);
    // The answer survives the reading the frame gives it.
    expect(readFromPptxWorker(structuredClone(answer))).toEqual(answer);
  });

  it('a relationship to outside the file is left out, whatever it names', async () => {
    const file = deck({
      slides: [
        {
          shapes: picture(2, 'rIdOutside', [0, 0, 1000, 1000]),
          links: [
            {
              id: 'rIdOutside',
              type: 'image',
              target: 'https://example.invalid/a.png',
              external: true,
            },
          ],
        },
      ],
    });
    const parts = await sorted(file);
    expect(parts.parts.slideRels.get('ppt/slides/_rels/slide1.xml.rels')).not.toContain(
      'example.invalid',
    );
  });

  it('pictures of kinds a browser draws without running anything are kept, others left out', async () => {
    const svg = Buffer.from(
      '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'.padEnd(64),
    );
    const emf = Buffer.alloc(64, 1);
    const file = deck({
      slides: [{ shapes: '' }],
      parts: [
        { name: 'media/image1.png', data: png(3, 2, [0, 0, 0]) },
        { name: 'media/image2.svg', data: svg },
        { name: 'media/image3.emf', data: emf },
      ],
    });
    const parts = await sorted(file);
    expect([...parts.parts.media.keys()]).toEqual(['ppt/media/image1.png']);
    expect(parts.pixels.get('ppt/media/image1.png')).toEqual({ width: 3, height: 2 });
  });
});

describe('each ceiling stops the deck that passes it, and only that one', () => {
  it('the Korean sample passes them all', async () => {
    expect(await stoppedAt(koreanPptx())).toBeUndefined();
  });

  for (const [ceiling, make] of Object.entries(HOSTILE_PPTX)) {
    it(ceiling, async () => {
      expect(await stoppedAt(make())).toBe(ceiling);
      const answer = await readPresentation(await repacked(make()));
      expect(answer).toEqual({ kind: 'failed', reason: 'too-large' });
    });
  }

  it("a slide is counted with its layout's shapes, which it draws", async () => {
    expect(await stoppedAt(layoutAndSlidePptx())).toBe('elements');
  });

  it('every ceiling has a deck', () => {
    for (const name of Object.keys(SLIDE_LIMITS)) expect(HOSTILE_PPTX).toHaveProperty([name]);
  });
});

describe('a part the frame would read otherwise than the worker stops the file', () => {
  it('a document type, with entities', async () => {
    expect(await readPresentation(await repacked(doctypePptx()))).toEqual({
      kind: 'failed',
      reason: 'unreadable',
    });
  });

  it('XML that is not well formed', async () => {
    expect(await readPresentation(await repacked(malformedPptx()))).toEqual({
      kind: 'failed',
      reason: 'unreadable',
    });
  });

  it('an archive without a presentation', async () => {
    const archive = zip([{ name: 'ppt/slides/slide1.xml', data: Buffer.from('<p:sld/>') }]);
    expect(await readPresentation(await repacked(archive))).toEqual({
      kind: 'failed',
      reason: 'unreadable',
    });
  });
});

describe('the size a picture decodes to is read from its header', () => {
  it('PNG', () => {
    expect(pixelsOf(png(7, 5, [0, 0, 0]))).toEqual({ width: 7, height: 5 });
    expect(pixelsOf(pngHeader(70_000, 50_000))).toEqual({ width: 70_000, height: 50_000 });
  });

  it('JPEG: its largest frame', () => {
    const segment = (marker: number, body: number[]): number[] => [
      0xff,
      marker,
      (body.length + 2) >> 8,
      (body.length + 2) & 0xff,
      ...body,
    ];
    const frame = (width: number, height: number): number[] =>
      segment(0xc0, [8, height >> 8, height & 0xff, width >> 8, width & 0xff, 1, 1, 0x11, 0]);
    const bytes = Uint8Array.from([
      0xff,
      0xd8,
      ...segment(
        0xe0,
        Array.from({ length: 14 }, () => 0),
      ),
      ...frame(10, 20),
      ...frame(3000, 2000),
      0xff,
      0xda,
      0,
      2,
    ]);
    expect(pixelsOf(bytes)).toEqual({ width: 3000, height: 2000 });
  });

  it('GIF: its screen grown to its frames', () => {
    const bytes = new Uint8Array(64);
    bytes.set(new TextEncoder().encode('GIF89a'));
    const view = new DataView(bytes.buffer);
    view.setUint16(6, 10, true);
    view.setUint16(8, 10, true);
    // No colour table; then an image of 500 by 40 at (100, 0).
    bytes[13] = 0x2c;
    view.setUint16(14, 100, true);
    view.setUint16(16, 0, true);
    view.setUint16(18, 500, true);
    view.setUint16(20, 40, true);
    expect(pixelsOf(bytes)).toEqual({ width: 600, height: 40 });
  });

  it('nothing else', () => {
    expect(
      pixelsOf(Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>'.padEnd(64))),
    ).toBeUndefined();
    expect(pixelsOf(new Uint8Array(8))).toBeUndefined();
  });
});

describe('a relationship leads where the renderer looks', () => {
  it('relative to its part, up and down', () => {
    expect(resolve('ppt/slides/slide1.xml', '../media/a.png')).toBe('ppt/media/a.png');
    expect(resolve('ppt/slides/slide1.xml', 'slide2.xml')).toBe('ppt/slides/slide2.xml');
    expect(resolve('ppt/slides/slide1.xml', '/ppt/media/b.png')).toBe('ppt/media/b.png');
  });
});

describe('the answer of the worker is taken only in its shape', () => {
  it('a failure, for one of its reasons', () => {
    expect(readFromPptxWorker({ kind: 'failed', reason: 'too-large' })).toEqual({
      kind: 'failed',
      reason: 'too-large',
    });
    expect(readFromPptxWorker({ kind: 'failed', reason: 'other' })).toBeUndefined();
  });

  it('not parts with a field of another type', async () => {
    const answer = await readPresentation(await repacked(koreanPptx()));
    if (answer.kind !== 'read') throw new Error('not read');
    const slides = new Map<string, unknown>([['ppt/slides/slide1.xml', 42]]);
    expect(readFromPptxWorker({ ...answer, parts: { ...answer.parts, slides } })).toBeUndefined();
    expect(
      readFromPptxWorker({ ...answer, parts: { ...answer.parts, media: new Map([['a', 'b']]) } }),
    ).toBeUndefined();
    expect(readFromPptxWorker({ ...answer, characters: 1 })).toBeUndefined();
    expect(readFromPptxWorker(null)).toBeUndefined();
  });
});
