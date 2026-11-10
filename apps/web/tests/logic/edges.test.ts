/**
 * The undo manager beyond the three cases of the original, and the
 * calculations at their edges. Written for the port. Cases named "quirk Qn"
 * pin a behaviour of the original that looks like a defect and is listed in
 * PORTING.md.
 */
import { describe, expect, it } from 'vitest';

import { CALCULATIONS } from '$lib/logic/calculations.ts';
import { UndoManager } from '$lib/logic/undo.ts';
import { createCard, setCardProperty, type Card, type PropertyTemplate } from '$lib/logic/wire.ts';

const nothing = async (): Promise<void> => undefined;

describe('undo manager', () => {
  it('tells its listener about every change of what can be undone', async () => {
    const manager = new UndoManager();
    let calls = 0;
    manager.onStateDidChange = () => {
      calls += 1;
    };
    await manager.perform(nothing, nothing);
    await manager.undo();
    await manager.redo();
    manager.clear();
    manager.clear(); // nothing left to clear: no call
    expect(calls).toBe(4);
  });

  it('forgets what was undone once a new change is made', async () => {
    const manager = new UndoManager();
    await manager.perform(nothing, nothing, 'first');
    await manager.perform(nothing, nothing, 'second');
    await manager.undo();
    expect(manager.redoDescription).toBe('second');
    await manager.perform(nothing, nothing, 'third');
    expect(manager.canRedo).toBe(false);
    expect(manager.undoDescription).toBe('third');
  });

  it('does not record a change that an undo itself makes', async () => {
    const manager = new UndoManager();
    await manager.perform(nothing, async () => {
      manager.registerUndo({ undo: nothing, redo: nothing }, 'made while undoing');
    });
    await manager.undo();
    expect(manager.canUndo).toBe(false);
    expect(manager.redoDescription).toBeUndefined();
  });

  it('keeps working after an undo failed (unlike the original, which stops for good)', async () => {
    const manager = new UndoManager();
    let fail = true;
    await manager.perform(nothing, async () => {
      if (fail) throw new Error('server said no');
    });
    await expect(manager.undo()).rejects.toThrow('server said no');
    // The failed change is still the one to undo, and it can be tried again.
    expect(manager.canUndo).toBe(true);
    fail = false;
    await manager.undo();
    expect(manager.canUndo).toBe(false);
  });

  it('gives a discardable change the checkpoint of the change before it', async () => {
    const manager = new UndoManager();
    await manager.perform(nothing, nothing, 'a');
    await manager.perform(nothing, nothing, 'b');
    const before = manager.currentCheckpoint;
    await manager.perform(nothing, nothing, 'typing', undefined, true);
    expect(manager.currentCheckpoint).toBe(before);
  });

  it('quirk Q14: a discardable change after a single change gets checkpoint 0', async () => {
    const manager = new UndoManager();
    await manager.perform(nothing, nothing, 'a');
    expect(manager.currentCheckpoint).toBeGreaterThan(0);
    await manager.perform(nothing, nothing, 'typing', undefined, true);
    expect(manager.currentCheckpoint).toBe(0);
  });
});

describe('number calculations at the edges', () => {
  const number: PropertyTemplate = { id: 'n', name: 'N', type: 'number', options: [] };
  const withNumber = (value: string): Card => {
    const card = createCard();
    setCardProperty(card, 'n', value);
    return card;
  };

  it('gives 0 for the minimum and maximum of no values', () => {
    expect(CALCULATIONS.min([], number, { locale: 'en' })).toBe('0');
    expect(CALCULATIONS.max([createCard()], number, { locale: 'en' })).toBe('0');
    expect(CALCULATIONS.range([], number, { locale: 'en' })).toBe('0 - 0');
  });

  it('shows an infinite value as it is', () => {
    expect(CALCULATIONS.max([withNumber('Infinity')], number, { locale: 'en' })).toBe('Infinity');
    expect(CALCULATIONS.min([withNumber('-Infinity')], number, { locale: 'en' })).toBe('-Infinity');
  });

  it('shows a value that is not a number as NaN', () => {
    expect(CALCULATIONS.sum([withNumber('abc'), withNumber('1')], number, { locale: 'en' })).toBe(
      'NaN',
    );
  });
});

describe('checkbox calculations', () => {
  const checkbox: PropertyTemplate = { id: 'done', name: 'Done', type: 'checkbox', options: [] };

  it('quirk Q15: the percentage of no cards is "NaN%"', () => {
    expect(CALCULATIONS.percentChecked([], checkbox, { locale: 'en' })).toBe('NaN%');
    expect(CALCULATIONS.percentUnchecked([], checkbox, { locale: 'en' })).toBe('NaN%');
    // The percentages of empty values do handle it.
    expect(CALCULATIONS.percentEmpty([], checkbox, { locale: 'en' })).toBe('');
  });
});
