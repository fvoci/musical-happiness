/**
 * The PowerPoint worker counts what the renderer will draw only if both find
 * the same parts: here the renderer builds its model (in the simulated page,
 * whose XML parser it uses) of the parts the worker answered with, and the
 * layout and master of each slide are the ones the worker counted it with
 * (src/viewer-frame/pptx-ceilings.ts), however the deck named them.
 */
import { buildPresentation } from '@aiden0z/pptx-renderer';
import { describe, expect, it } from 'vitest';

import { drawnWith } from '../../src/viewer-frame/pptx-ceilings.ts';
import { sortParts } from '../../src/viewer-frame/pptx-parts.ts';
import { unpack } from '../../src/viewer-frame/pptx-unpack.ts';
import { repack } from '../../src/viewer-frame/zip-guard.ts';

import { layoutAndSlidePptx, smartArtPptx } from '../support/hostile-pptx.ts';
import { deck, koreanPptx, NAMESPACES } from '../support/viewer-pptx.ts';

/** A layout of the deck that the slide names in a way PowerPoint does not write. */
const twoLayouts = (type: string, target: string): Buffer =>
  deck({
    slides: [
      {
        shapes: '',
        layout: { id: 'rIdOther', type, target },
      },
    ],
    parts: [
      {
        name: 'slideLayouts/slideLayout2.xml',
        data: `<p:sldLayout ${NAMESPACES}><p:cSld><p:spTree/></p:cSld></p:sldLayout>`,
      },
    ],
  });

const DECKS: Record<string, () => Buffer> = {
  'the Korean sample': koreanPptx,
  'a layout named with a query': () => layoutAndSlidePptx('../slideLayouts/slideLayout1.xml?x#y'),
  'a layout named with backslashes': () => layoutAndSlidePptx('..\\slideLayouts\\slideLayout1.xml'),
  'a layout named with an escape': () => layoutAndSlidePptx('../slideLayouts/slide%4Cayout1.xml'),
  'a layout named from the root': () => layoutAndSlidePptx('/ppt/slideLayouts/./slideLayout1.xml'),
  'a layout of a type that only has the name in it': () =>
    twoLayouts('slideLayouts', '../slideLayouts/slideLayout2.xml'),
  'a layout that is not there': () => twoLayouts('slideLayout', '../slideLayouts/slideLayout9.xml'),
  SmartArt: () => smartArtPptx(3, 2),
};

describe('the renderer draws each slide with the layout and master the worker counted', () => {
  for (const [name, make] of Object.entries(DECKS)) {
    it(name, async () => {
      const sorted = sortParts(await unpack(await repack(Uint8Array.from(make()).buffer)));
      const presentation = buildPresentation(sorted.parts, { lazySlides: true });
      expect(presentation.slides.length).toBeGreaterThan(0);
      for (const [index, slide] of presentation.slides.entries()) {
        const { layout, master } = drawnWith(sorted, slide.slidePath);
        const drawnLayout = presentation.slideToLayout.get(index);
        expect(drawnLayout).toBe(layout);
        expect(
          drawnLayout === undefined ? undefined : presentation.layoutToMaster.get(drawnLayout),
        ).toBe(master);
      }
    });
  }
});
