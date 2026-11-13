/** Reserving a gesture is not a change that can discard an earlier undo. */
import { expect, it } from 'vitest';

import { UndoManager } from '$lib/logic/undo.ts';

function held() {
  let release = (): void => undefined;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

async function adding(
  manager: UndoManager,
  values: string[],
  value: string,
  groupId?: string,
  gate?: Promise<void>,
) {
  await manager.perform(
    async () => {
      values.push(value);
      await gate;
    },
    async () => {
      values.splice(values.indexOf(value), 1);
    },
    value,
    groupId,
  );
}

it.each([false, true])(
  'keeps an earlier undo when a reserved gesture changes nothing (throws: %s)',
  async (throws) => {
    const manager = new UndoManager();
    const values: string[] = [];
    const write = held();
    const read = held();
    const changing = adding(manager, values, 'A', undefined, write.promise);
    const undoing = manager.undo();
    const reservation = manager
      .group(async () => {
        await read.promise;
        if (throws) throw new Error('unread');
      }, true)
      .catch(() => undefined);
    read.release();
    await reservation;
    write.release();
    await Promise.all([changing, undoing]);
    expect(values).toEqual([]);
    expect(manager.canUndo).toBe(false);
    expect(manager.redoDescription).toBe('A');
  },
);

it('keeps multiple queued presses around two empty reservations', async () => {
  const manager = new UndoManager();
  const values: string[] = [];
  await adding(manager, values, 'before');
  const write = held();
  const read = held();
  const changing = adding(manager, values, 'A', undefined, write.promise);
  const first = manager.undo();
  const reservations = [
    manager.group(() => read.promise, true),
    manager.group(() => read.promise, true),
  ];
  const second = manager.undo();
  const third = manager.undo();
  const fourth = manager.undo();
  read.release();
  write.release();
  await Promise.all([changing, first, ...reservations, second, third, fourth]);
  expect(values).toEqual([]);
  expect(manager.canUndo).toBe(false);
  expect(manager.busy).toBe(false);
  await manager.redo();
  expect(values).toEqual(['before']);
  await manager.redo();
  expect(values).toEqual(['before', 'A']);
});

it('keeps an earlier redo when an empty reservation ends after that redo', async () => {
  const manager = new UndoManager();
  const values: string[] = [];
  await adding(manager, values, 'A');
  await manager.undo();
  const redoing = manager.redo();
  const read = held();
  const reservation = manager.group(() => read.promise, true);
  await redoing;
  read.release();
  await reservation;
  expect(values).toEqual(['A']);
  await manager.undo();
  expect(values).toEqual([]);
});

it('drops only earlier waiting requests when a reservation really changes something', async () => {
  const manager = new UndoManager();
  const values: string[] = [];
  const write = held();
  const read = held();
  const changing = adding(manager, values, 'A', undefined, write.promise);
  const earlier = manager.undo();
  const reservation = manager.group(async (groupId) => {
    await read.promise;
    await adding(manager, values, 'copy', groupId);
  }, true);
  const undoCopy = manager.undo();
  read.release();
  await Promise.all([reservation, earlier, undoCopy]);
  expect(values).toEqual(['A']);
  write.release();
  await changing;
  expect(manager.undoDescription).toBe('A');
  expect(manager.redoDescription).toBe('copy');
});

it('keeps the retry of a failed redo when a reservation then ends empty', async () => {
  const manager = new UndoManager();
  const write = held();
  const read = held();
  let fail = false;
  await manager.perform(
    async () => {
      if (fail) {
        await write.promise;
        throw new Error('refused');
      }
    },
    async () => undefined,
    'A',
  );
  await manager.undo();
  fail = true;
  const redoing = manager.redo().catch(() => undefined);
  // Allow the redo to begin before the reservation.
  await new Promise((resolve) => {
    setTimeout(resolve);
  });
  const reservation = manager.group(() => read.promise, true);
  write.release();
  await redoing;
  read.release();
  await reservation;
  expect(manager.redoDescription).toBe('A');
  fail = false;
  await manager.redo();
  expect(manager.undoDescription).toBe('A');
});

it('cuts an earlier completed undo only once the reservation makes its change', async () => {
  const manager = new UndoManager();
  const values: string[] = [];
  const read = held();
  await adding(manager, values, 'A');
  const undoing = manager.undo();
  const reservation = manager.group(async (groupId) => {
    await read.promise;
    await adding(manager, values, 'copy', groupId);
  }, true);
  await undoing;
  expect(values).toEqual([]);
  read.release();
  await reservation;
  await manager.undo();
  expect(manager.redoDescription).toBe('copy');
  await manager.redo();
  expect(values).toEqual(['copy']);
  expect(manager.canRedo).toBe(false);
});

it('cuts a redo cancelled by the first real change of a reservation', async () => {
  const manager = new UndoManager();
  const values: string[] = [];
  await adding(manager, values, 'A');
  await manager.undo();
  const redoing = manager.redo();
  const reservation = manager.group((groupId) => adding(manager, values, 'copy', groupId), true);
  await Promise.all([redoing, reservation]);
  expect(values).toEqual(['copy']);
  await manager.undo();
  expect(manager.redoDescription).toBe('copy');
  await manager.redo();
  expect(values).toEqual(['copy']);
});

it('cuts a failed redo retry when the reservation subsequently makes a change', async () => {
  const manager = new UndoManager();
  const values: string[] = [];
  const started = held();
  const write = held();
  const read = held();
  let fail = false;
  await manager.perform(
    async () => {
      if (fail) {
        started.release();
        await write.promise;
        throw new Error('refused');
      }
      values.push('A');
    },
    async () => {
      values.pop();
    },
    'A',
  );
  await manager.undo();
  fail = true;
  const redoing = manager.redo().catch(() => undefined);
  await started.promise;
  const reservation = manager.group(async (groupId) => {
    await read.promise;
    await adding(manager, values, 'copy', groupId);
  }, true);
  write.release();
  await redoing;
  read.release();
  await reservation;
  await manager.undo();
  expect(manager.redoDescription).toBe('copy');
  await manager.redo();
  expect(values).toEqual(['copy']);
});
