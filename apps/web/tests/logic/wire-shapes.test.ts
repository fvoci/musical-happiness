/**
 * The parts of wire.ts the ported tests do not reach: reading stored date
 * values, filling in blocks, identifiers. Written for the port. Like
 * wire.test.ts, this file may look inside the `fields` of a block.
 */
import { describe, expect, it } from 'vitest';

import { calendarDate } from '$lib/logic/dates.ts';

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
  formatFilterDate,
  idKindOfBlockType,
  normalizeBlock,
  parseDateValue,
  parseFilterDate,
  type Board,
} from '$lib/logic/wire.ts';

describe('the text of a date property', () => {
  const noon = (date: number): number => Date.UTC(2026, 2, date, 12);

  it('reads a day from its noon in UTC', () => {
    expect(parseDateValue(`{"from":${noon(5)}}`)).toEqual({ from: { date: '2026-03-05' } });
    expect(parseDateValue(`{"from":${noon(5)},"to":${noon(9)}}`)).toEqual({
      from: { date: '2026-03-05' },
      to: { date: '2026-03-09' },
    });
  });

  it('reads the day even when the stored time is not exactly noon (Q11)', () => {
    expect(parseDateValue(`{"from":${noon(5) + 17 * 60_000}}`)).toEqual({
      from: { date: '2026-03-05' },
    });
    expect(parseDateValue(`{"from":${Date.UTC(2026, 2, 5, 0, 0)}}`)).toEqual({
      from: { date: '2026-03-05' },
    });
  });

  it('reads a value with a time as the moment it is', () => {
    const moment = Date.UTC(2026, 2, 5, 18, 30);
    expect(parseDateValue(`{"from":${moment},"includeTime":true}`)).toEqual({
      from: { instant: moment },
    });
  });

  it('writes a day as exactly its noon in UTC, whatever the time zone of the device', () => {
    const value = {
      from: { date: calendarDate(2026, 3, 5) },
      to: { date: calendarDate(2026, 3, 9) },
    };
    expect(formatDateValue(value)).toBe(`{"from":${noon(5)},"to":${noon(9)}}`);
    expect(parseDateValue(formatDateValue(value))).toEqual(value);
    const timed = { from: { instant: 1_780_000_000_000 } };
    expect(parseDateValue(formatDateValue(timed))).toEqual(timed);
  });

  it('reads anything else as an empty value', () => {
    for (const text of [
      undefined,
      '',
      'tomorrow',
      'null',
      '"text"',
      '[1,2]',
      '{}',
      '{"from":"abc"}',
      '{"from":0}',
    ]) {
      expect(parseDateValue(text)).toEqual({});
    }
    // The bare number of very old data is not read any more (Q10).
    expect(parseDateValue('1625553000000')).toEqual({});
  });

  it('keeps the end of a value that has lost its start', () => {
    expect(parseDateValue(`{"to":${noon(9)}}`)).toEqual({ from: { date: '2026-03-09' } });
  });

  it('writes nothing for an empty value', () => {
    expect(formatDateValue({})).toBe('');
  });

  it('reads and writes the day of a date filter the same way', () => {
    expect(parseFilterDate(String(noon(5)))).toBe('2026-03-05');
    expect(formatFilterDate(calendarDate(2026, 3, 5))).toBe(String(noon(5)));
    expect(parseFilterDate('soon')).toBeUndefined();
    expect(parseFilterDate(undefined)).toBeUndefined();
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
