/**
 * The parts of wire.ts the ported tests do not reach: reading stored date
 * values, filling in blocks, identifiers. Written for the port. Like
 * wire.test.ts, this file may look inside the `fields` of a block.
 */
import { describe, expect, it } from 'vitest';

import {
  createAttachmentBlock,
  createBlock,
  createBoard,
  createBoardView,
  createCard,
  createFilterClause,
  createPatchesFromBoards,
  encodeBase32,
  formatDateValue,
  idKindOfBlockType,
  normalizeBlock,
  parseDateObject,
  parseDateValue,
  type Board,
} from '$lib/logic/wire.ts';

describe('the text of a date property', () => {
  it('reads the object form and the older bare number', () => {
    expect(parseDateValue('{"from":10,"to":20}')).toEqual({ from: 10, to: 20 });
    expect(parseDateValue('1625553000000')).toEqual({ from: 1625553000000 });
  });

  it('reads anything else as an empty value', () => {
    expect(parseDateValue(undefined)).toEqual({});
    expect(parseDateValue('')).toEqual({});
    expect(parseDateValue('tomorrow')).toEqual({});
  });

  it('reads JSON that is not an object as an empty value', () => {
    expect(parseDateValue('null')).toEqual({});
    expect(parseDateValue('"text"')).toEqual({});
    expect(parseDateValue('[1,2]')).toEqual({});
  });

  it('writes nothing for a value without a start or an end', () => {
    expect(formatDateValue({})).toBe('');
    expect(formatDateValue({ includeTime: true })).toBe('');
    expect(parseDateValue(formatDateValue({ from: 5, to: 9 }))).toEqual({ from: 5, to: 9 });
  });

  it('quirk Q10: the strict reader does not take the bare number', () => {
    expect(parseDateObject('1625553000000')).toEqual({});
    expect(parseDateObject('{"from":10}')).toEqual({ from: 10 });
    expect(parseDateObject('nonsense')).toEqual({});
  });
});

describe('stored data with gaps', () => {
  it('fills a filter clause that has no values', () => {
    expect(createFilterClause({ propertyId: 'x', condition: 'isEmpty' })).toEqual({
      propertyId: 'x',
      condition: 'isEmpty',
      values: [],
    });
    const view = createBoardView({
      ...createBlock(),
      fields: { filter: { operation: 'or', filters: [{ propertyId: 'x', condition: 'isSet' }] } },
    });
    expect(view.fields.filter.filters).toEqual([
      { propertyId: 'x', condition: 'isSet', values: [] },
    ]);
  });

  it('makes board patches when a board has no properties object', () => {
    const board = createBoard();
    const bare = { ...board, properties: undefined } as unknown as Board;
    const [patch, undo] = createPatchesFromBoards({ ...board, properties: { a: '1' } }, bare);
    expect(patch.updatedProperties).toEqual({ a: '1' });
    expect(undo.deletedProperties).toEqual(['a']);
    expect(createPatchesFromBoards(bare, bare)[0].updatedProperties).toEqual({});
  });
});

describe('blocks', () => {
  it('fills the gaps of a block and copies its fields', () => {
    const source = { ...createBlock(), id: 'x', title: 't', fields: { a: 1 } };
    const copy = createBlock(source);
    expect(copy).toEqual({ ...source, limited: false });
    expect(copy.fields).not.toBe(source.fields);
    expect(createBlock().id).toMatch(/^7/);
  });

  it('drops empty entries of a content order and copies its rows', () => {
    const row = ['b', 'c'];
    const card = createCard({ ...createBlock(), fields: { contentOrder: ['a', null, row] } });
    expect(card.type).toBe('card');
    expect(card.fields.contentOrder).toEqual(['a', ['b', 'c']]);
    expect(card.fields.contentOrder[1]).not.toBe(row);
    expect(card.fields.properties).toEqual({});
  });

  it('reads the file of an attachment under its older name too', () => {
    const old = createAttachmentBlock({ ...createBlock(), fields: { attachmentId: 'file-1' } });
    expect(old.fields).toEqual({ fileId: 'file-1' });
  });

  it('gives a block of a known type the members of that type', () => {
    const view = normalizeBlock({ ...createBlock(), type: 'view' });
    expect(view.fields.viewType).toBe('board');
    expect(view.fields.filter).toEqual({ operation: 'and', filters: [] });
  });

  it('names the kind of identifier for a block type', () => {
    expect(['card', 'view', 'board', 'text', undefined].map(idKindOfBlockType)).toEqual([
      'card',
      'view',
      'board',
      'none',
      'none',
    ]);
  });

  it('encodes bytes in z-base-32', () => {
    expect(encodeBase32(new Uint8Array([0, 0, 0, 0, 0]))).toBe('yyyyyyyy');
    expect(encodeBase32(new Uint8Array([255]))).toBe('9h');
    expect(encodeBase32(new Uint8Array([255]), true)).toBe('9h======');
  });
});
