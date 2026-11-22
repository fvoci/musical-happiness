/**
 * The body a card's or a page's document in its room holds now, as it would
 * be stored: what the reader and the others typed, which the stored body has
 * only once whoever holds the room writes it. Fetched on its own where it is
 * needed (the comparison of a version with the body now), so that what reads
 * Yjs's document comes with the editor and no sooner.
 */
import { yXmlFragmentToProsemirrorJSON } from '@tiptap/y-tiptap';
import type { Doc } from 'yjs';

import { BODY_FIELD } from './body-editor.ts';
import { isEmptyDoc, toStoredDoc, validateDoc, type DocNode } from './schema.ts';

/** The body a document holds, as it would be stored; undefined when it is not one. */
export function bodyOfDoc(doc: Doc): DocNode | null | undefined {
  try {
    const fragment = doc.getXmlFragment(BODY_FIELD);
    const stored = toStoredDoc(yXmlFragmentToProsemirrorJSON(fragment) as DocNode);
    if (validateDoc(stored).length > 0) return undefined;
    return isEmptyDoc(stored) ? null : stored;
  } catch {
    return undefined;
  }
}
