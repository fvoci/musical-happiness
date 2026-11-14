/**
 * Where a place of the reader's is after the others changed the body
 * (`placeAfter`, editor/moves-kept.ts): the selection the reader moved to
 * with a key is carried through such a change by it.
 */
import { getSchema } from '@tiptap/core';
import type { Node } from '@tiptap/pm/model';
import { describe, expect, it } from 'vitest';

import { bodyExtensions } from '$lib/editor/extensions.ts';
import { placeAfter } from '$lib/editor/moves-kept.ts';

const schema = getSchema(bodyExtensions());

/** A body of paragraphs, one for each line. */
const body = (...lines: string[]): Node =>
  schema.nodeFromJSON({
    type: 'doc',
    content: lines.map((line) => ({
      type: 'paragraph',
      ...(line ? { content: [{ type: 'text', text: line }] } : {}),
    })),
  });

/** What stands behind a position, to its line's end. */
const behind = (doc: Node, position: number): string => {
  const $place = doc.resolve(position);
  return $place.parent.textContent.slice($place.parentOffset);
};

describe('where a place is after the others changed the body', () => {
  const before = body('abcdef', 'Below.');

  it('is itself when nothing changed', () => {
    expect(placeAfter(before, body('abcdef', 'Below.'), 4)).toBe(4);
  });

  it('is itself ahead of what changed, and where the change begins', () => {
    const after = body('abcdef', 'XYBelow.');
    expect(placeAfter(before, after, 4)).toBe(4);
    expect(placeAfter(before, after, 9)).toBe(9);
  });

  it('moves with what was written ahead of it', () => {
    const after = body('abXYcdef', 'Below.');
    for (const position of [4, 6, 7, 9, 12, 15]) {
      const moved = placeAfter(before, after, position);
      expect(moved).toBe(position + 2);
      expect(behind(after, moved ?? 0)).toBe(behind(before, position));
    }
    expect(placeAfter(before, after, 2)).toBe(2);
  });

  it('moves back with what was deleted ahead of it, and has no place within it', () => {
    const after = body('abef', 'Below.');
    expect(placeAfter(before, after, 4)).toBeUndefined();
    expect(placeAfter(before, after, 5)).toBe(3);
    expect(placeAfter(before, after, 12)).toBe(10);
  });

  it('moves with a line written above it', () => {
    const after = body('new', 'abcdef', 'Below.');
    const moved = placeAfter(before, after, 3);
    expect(moved).toBe(8);
    expect(behind(after, moved ?? 0)).toBe('cdef');
  });

  it('is not misled by the same letters on both sides of what was written', () => {
    const letters = body('aaa', 'Below.');
    const more = body('aaaa', 'Below.');
    expect(placeAfter(letters, more, 2)).toBe(2);
    expect(placeAfter(letters, more, 4)).toBe(4);
    const moved = placeAfter(letters, more, 8);
    expect(moved).toBe(9);
    expect(behind(more, moved ?? 0)).toBe(behind(letters, 8));
  });
});
