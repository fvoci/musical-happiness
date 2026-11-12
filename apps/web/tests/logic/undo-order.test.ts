/**
 * Undo and redo asked for while writes are under way: a change is a step
 * from the moment it is made, undo and redo wait their turn, and a change
 * that is refused leaves nothing for them to do.
 */
import { describe, expect, it } from 'vitest';

import { UndoManager } from '$lib/logic/undo.ts';

/** A write held until the test answers it. */
interface Held {
  promise: Promise<void>;
  answer: () => void;
  refuse: () => void;
}

function held(): Held {
  let answer = (): void => undefined;
  let refuse = (): void => undefined;
  const promise = new Promise<void>((resolve, reject) => {
    answer = resolve;
    refuse = () => {
      reject(new Error('refused'));
    };
  });
  return { promise, answer, refuse };
}

/** A value changed through a backend whose every write the test holds. */
function setup() {
  const manager = new UndoManager();
  /** What the screen shows: changed at once, put back when a write is refused. */
  let shown = 'start';
  /** What the backend has stored. */
  let saved = 'start';
  const writes: Held[] = [];
  const write = async (value: string): Promise<void> => {
    const before = shown;
    shown = value;
    const one = held();
    writes.push(one);
    try {
      await one.promise;
      saved = value;
    } catch (error) {
      shown = before;
      throw error;
    }
  };
  /**
   * Changes the value as a step of undo, from what is shown when it is made;
   * a refusal is caught, as the stores do.
   */
  const change = async (to: string): Promise<void> => {
    let from: string | undefined;
    await manager
      .perform(
        () => {
          from ??= shown;
          return write(to);
        },
        () => write(from ?? 'start'),
      )
      .catch(() => undefined);
  };
  /** Lets every promise that can go on do so: a task runs after all of them. */
  const flush = (): Promise<void> =>
    new Promise((resolve) => {
      setTimeout(resolve);
    });
  return {
    manager,
    change,
    flush,
    writes,
    shown: () => shown,
    saved: () => saved,
    /** Answers the write at `at` among those sent and not answered yet. */
    answerAt: async (at: number) => {
      await flush();
      writes.splice(at, 1)[0]?.answer();
      await flush();
    },
    /** Answers the oldest write sent and not answered yet. */
    answer: async () => {
      await flush();
      writes.shift()?.answer();
      await flush();
    },
    refuse: async () => {
      await flush();
      writes.shift()?.refuse();
      await flush();
    },
  };
}

describe('undo while a change is being written', () => {
  it('takes back that change once it is written, not the one before', async () => {
    const { manager, change, answer, shown, saved, writes } = setup();
    void change('one');
    await answer();
    void change('two');
    const undone = manager.undo();
    // Asked for at once: "one" is the next to undo, "two" the next to redo.
    expect(manager.canRedo).toBe(true);
    expect(manager.busy).toBe(true);
    expect(shown()).toBe('two');
    expect(writes).toHaveLength(1);
    await answer(); // "two" is written; the undo sends "one".
    expect(shown()).toBe('one');
    await answer();
    await undone;
    expect(saved()).toBe('one');
    expect(manager.busy).toBe(false);
    expect(manager.canUndo).toBe(true);
  });

  it('drops an undo that waited for a refused change, and the change before stays', async () => {
    const { manager, change, answer, refuse, shown, saved, writes } = setup();
    void change('one');
    await answer();
    void change('two');
    const undone = manager.undo();
    await refuse();
    await undone;
    expect(shown()).toBe('one');
    expect(saved()).toBe('one');
    // Nothing more was sent: "one" was not taken back.
    expect(writes).toHaveLength(0);
    expect(manager.canUndo).toBe(true);
    expect(manager.canRedo).toBe(false);
    expect(manager.busy).toBe(false);
  });

  it('undoes changes one after the other, each when its write is back, in any order of answers', async () => {
    const { manager, change, answerAt, shown, saved, writes } = setup();
    void change('one');
    void change('two');
    const first = manager.undo();
    const second = manager.undo();
    expect(manager.canUndo).toBe(false);
    // "two" is answered before "one": its undo goes out; the undo of "one" waits for "one".
    await answerAt(1);
    expect(writes).toHaveLength(2);
    await answerAt(1);
    expect(writes).toHaveLength(1);
    await answerAt(0);
    expect(writes).toHaveLength(1);
    await answerAt(0);
    await Promise.all([first, second]);
    expect(shown()).toBe('start');
    expect(saved()).toBe('start');
    expect(manager.canRedo).toBe(true);
  });

  it('keeps a second undo asked for meanwhile for the step before the refused one', async () => {
    const { manager, change, answer, refuse, shown, writes } = setup();
    void change('one');
    await answer();
    void change('two');
    const first = manager.undo();
    const second = manager.undo();
    await refuse();
    // The undo of "two" is dropped; the second was asked for "one", and takes it back.
    expect(writes).toHaveLength(1);
    await answer();
    await Promise.all([first, second]);
    expect(shown()).toBe('start');
    expect(manager.canUndo).toBe(false);
    expect(manager.canRedo).toBe(true);
  });

  it('makes a change asked for while an undo waits after that undo, and from what it left', async () => {
    const { manager, change, answer, shown, saved, writes } = setup();
    void change('one');
    await answer();
    void change('two');
    const undone = manager.undo();
    const made = change('three');
    // "three" is the newest step at once, and waits for the undo asked for before it.
    expect(manager.canRedo).toBe(false);
    expect(manager.canUndo).toBe(true);
    expect(writes).toHaveLength(1);
    await answer(); // "two"; the undo of "two" is sent
    expect(shown()).toBe('one');
    await answer(); // the undo of "two"; "three" is sent
    await undone;
    expect(shown()).toBe('three');
    await answer();
    await made;
    expect(saved()).toBe('three');
    // Undo of "three" goes back to what it was made from: "one".
    const again = manager.undo();
    await answer();
    await again;
    expect(saved()).toBe('one');
    expect(manager.canUndo).toBe(true);
  });
});

describe('redo while an undo is being written', () => {
  it('waits for the undo, then does the change again', async () => {
    const { manager, change, answer, shown, saved } = setup();
    void change('one');
    await answer();
    const undone = manager.undo();
    const redone = manager.redo();
    expect(manager.canRedo).toBe(false);
    await answer();
    expect(shown()).toBe('one');
    await answer();
    await Promise.all([undone, redone]);
    expect(saved()).toBe('one');
    expect(manager.canUndo).toBe(true);
  });

  it('is dropped when the undo is refused, and the change stays in force', async () => {
    const { manager, change, answer, refuse, shown, writes } = setup();
    void change('one');
    await answer();
    const undone = manager.undo().catch((error: unknown) => error);
    const redone = manager.redo();
    await refuse();
    expect(await undone).toEqual(new Error('refused'));
    await redone;
    expect(writes).toHaveLength(0);
    expect(shown()).toBe('one');
    expect(manager.canUndo).toBe(true);
    expect(manager.canRedo).toBe(false);
  });
});

describe('a gesture of several changes', () => {
  it('is one step from its start, and an undo asked for meanwhile waits for all of it', async () => {
    const manager = new UndoManager();
    const values: string[] = [];
    const gate = held();
    const push = (value: string) => () =>
      manager.perform(
        async () => {
          values.push(value);
        },
        async () => {
          values.splice(values.indexOf(value), 1);
        },
        value,
        undefined,
      );
    await push('before')();
    const gesture = manager.group(async (groupId) => {
      await manager.perform(
        async () => {
          values.push('a');
        },
        async () => {
          values.splice(values.indexOf('a'), 1);
        },
        'a',
        groupId,
      );
      await gate.promise;
      await manager.perform(
        async () => {
          values.push('b');
        },
        async () => {
          values.splice(values.indexOf('b'), 1);
        },
        'b',
        groupId,
      );
    });
    // Another change made while the gesture is under way comes after it.
    await push('between')();
    const first = manager.undo();
    const second = manager.undo();
    gate.answer();
    await gesture;
    await Promise.all([first, second]);
    expect(values).toEqual(['before']);
    expect(manager.undoDescription).toBe('before');
    expect(manager.redoDescription).toBe('a');
  });

  it('leaves no step when it changed nothing', async () => {
    const manager = new UndoManager();
    await manager.perform(
      async () => undefined,
      async () => undefined,
      'before',
    );
    await manager.group(async () => undefined);
    expect(manager.undoDescription).toBe('before');
  });
});
