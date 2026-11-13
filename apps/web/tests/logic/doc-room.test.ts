/**
 * One room of a body as its client sees it (`logic/doc-room.ts`): what it
 * answers to each thing the server says, the four times the document is
 * received again, and how changes are gathered into frames.
 */
import { describe, expect, it } from 'vitest';
import * as Y from 'yjs';

import {
  DocRoom,
  FLUSH_WAIT,
  PACE_WAIT,
  RESET_WAIT,
  type RoomEffect,
  type RoomEvent,
  type RoomFrame,
} from '$lib/logic/doc-room.ts';

const WHOLE = new Uint8Array([0, 0]);
const STEP_2 = 1;
const UPDATE = 2;

/** An update of `size` bytes of text, made on a document of its own. */
function update(size = 10): Uint8Array {
  const doc = new Y.Doc();
  doc.getText('t').insert(0, 'x'.repeat(size));
  return Y.encodeStateAsUpdate(doc);
}

class Harness {
  readonly room = new DocRoom((updates) => Y.mergeUpdates(updates), { updateBytes: 1024 });
  effects: RoomEffect[] = [];

  take(event: RoomEvent): RoomEffect[] {
    this.effects = this.room.take(event);
    return this.effects;
  }

  of<T extends RoomEffect['type']>(type: T): Extract<RoomEffect, { type: T }>[] {
    return this.effects.filter(
      (effect): effect is Extract<RoomEffect, { type: T }> => effect.type === type,
    );
  }

  sent(): RoomFrame[] {
    return this.of('send').map((effect) => effect.frame);
  }

  /** Up, joined as room `doc` of `epoch`, and the whole document received. */
  joined(doc = 1, epoch = 7, mode: 'write' | 'read' = 'write'): void {
    this.take({ type: 'up' });
    this.take({ type: 'message', message: { action: 'DOC_JOINED', doc, epoch, mode } });
    this.take({ type: 'frame', frame: { doc, epoch, sync: STEP_2, payload: WHOLE } });
  }

  /** A local change, gathered and sent. */
  write(payload = update()): RoomFrame[] {
    this.take({ type: 'local', update: payload });
    this.take({ type: 'timer', timer: 'flush' });
    return this.sent();
  }
}

describe('joining', () => {
  it('asks once the connection is there, and holds the server whole document first', () => {
    const room = new Harness();
    expect(room.take({ type: 'local', update: update() })).toEqual([]);
    expect(room.take({ type: 'up' }).map((effect) => effect.type)).toEqual([
      'cancel',
      'join',
      'changed',
    ]);
    expect(room.room.status.phase).toBe('joining');
    room.take({
      type: 'message',
      message: { action: 'DOC_JOINED', doc: 4, epoch: 2, mode: 'write' },
    });
    expect(room.room.status.writable).toBe(false);
    // A frame of another room, then an update before the whole document: dropped.
    expect(
      room.take({ type: 'frame', frame: { doc: 5, epoch: 2, sync: STEP_2, payload: WHOLE } }),
    ).toEqual([]);
    expect(
      room.take({ type: 'frame', frame: { doc: 4, epoch: 2, sync: UPDATE, payload: WHOLE } }),
    ).toEqual([]);
    room.take({ type: 'frame', frame: { doc: 4, epoch: 2, sync: STEP_2, payload: WHOLE } });
    expect(room.of('fresh')).toEqual([{ type: 'fresh', update: WHOLE }]);
    expect(room.room.status).toMatchObject({
      phase: 'joined',
      mode: 'write',
      writable: true,
      held: true,
    });
  });

  it('is read only when let in to read, and a change made then is never sent', () => {
    const room = new Harness();
    room.joined(1, 1, 'read');
    expect(room.room.status.writable).toBe(false);
    expect(room.write()).toEqual([]);
    expect(room.room.status.pending).toBe(false);
  });

  it('is refused for good when the card is not there for the reader, and its document let go', () => {
    const room = new Harness();
    room.joined();
    room.take({ type: 'message', message: { action: 'DOC_LEFT', doc: 1, reason: 'not_found' } });
    expect(room.room.status).toMatchObject({
      phase: 'out',
      reason: 'not_found',
      retrying: false,
      held: false,
    });
    expect(room.of('drop')).toHaveLength(1);
  });

  it('tries again, later each time, what is refused for now', () => {
    const room = new Harness();
    room.take({ type: 'up' });
    const waits: number[] = [];
    for (let attempt = 0; attempt < 3; attempt += 1) {
      room.take({ type: 'message', message: { action: 'DOC_REFUSED', reason: 'document_full' } });
      const timer = room.of('timer')[0];
      waits.push(timer?.wait ?? 0);
      expect(room.room.status).toMatchObject({ phase: 'out', retrying: true });
      expect(
        room.take({ type: 'timer', timer: 'retry' }).some((effect) => effect.type === 'join'),
      ).toBe(true);
    }
    expect(waits).toEqual([1000, 2000, 5000]);
  });
});

describe('the frames of a room', () => {
  it('drops those of an old epoch and of a room not known', () => {
    const room = new Harness();
    room.joined(1, 7);
    expect(
      room.take({ type: 'frame', frame: { doc: 1, epoch: 6, sync: UPDATE, payload: WHOLE } }),
    ).toEqual([]);
    expect(
      room.take({ type: 'frame', frame: { doc: 2, epoch: 7, sync: UPDATE, payload: WHOLE } }),
    ).toEqual([]);
    expect(
      room.take({ type: 'frame', frame: { doc: 1, epoch: 7, sync: UPDATE, payload: WHOLE } }),
    ).toEqual([{ type: 'apply', update: WHOLE }]);
    // Answers of a room left: dropped.
    expect(
      room.take({
        type: 'message',
        message: { action: 'DOC_NACK', doc: 2, epoch: 7, reason: 'update_refused' },
      }),
    ).toEqual([]);
  });
});

describe('gathering changes', () => {
  it('sends the changes of one wait as one frame, and the next frame after a pause', () => {
    const room = new Harness();
    room.joined();
    const first = room.take({ type: 'local', update: update(3) });
    expect(first).toContainEqual({ type: 'timer', timer: 'flush', wait: FLUSH_WAIT });
    expect(room.take({ type: 'local', update: update(4) })).toEqual([]);
    room.take({ type: 'timer', timer: 'flush' });
    expect(room.sent()).toHaveLength(1);
    expect(room.room.status.pending).toBe(true);
    room.take({ type: 'message', message: { action: 'DOC_ACK', doc: 1, epoch: 7 } });
    expect(room.room.status.pending).toBe(false);
  });

  it('cuts what is over the limit into frames under it, sent one a wait', () => {
    const room = new Harness();
    room.joined();
    for (let index = 0; index < 6; index += 1) room.take({ type: 'local', update: update(300) });
    room.take({ type: 'timer', timer: 'flush' });
    const frames = [...room.sent()];
    expect(frames).toHaveLength(1);
    expect(room.of('timer')).toEqual([{ type: 'timer', timer: 'flush', wait: FLUSH_WAIT }]);
    while (room.of('timer').length > 0) {
      room.take({ type: 'timer', timer: 'flush' });
      frames.push(...room.sent());
    }
    expect(frames.length).toBeGreaterThan(1);
    for (const frame of frames) expect(frame.payload.length).toBeLessThanOrEqual(1024);
  });

  it('cannot keep one change larger than the server takes: the document is received again and the loss said', () => {
    const room = new Harness();
    room.joined();
    room.take({ type: 'local', update: update(2000) });
    room.take({ type: 'timer', timer: 'flush' });
    expect(room.sent()).toEqual([]);
    expect(room.of('join')).toHaveLength(1);
    room.take({
      type: 'message',
      message: { action: 'DOC_JOINED', doc: 2, epoch: 7, mode: 'write' },
    });
    room.take({ type: 'frame', frame: { doc: 2, epoch: 7, sync: STEP_2, payload: WHOLE } });
    expect(room.of('discarded')).toEqual([{ type: 'discarded', why: 'update_too_large' }]);
    expect(room.of('fresh')).toHaveLength(1);
  });

  it('sends everything waiting at once when told to settle', () => {
    const room = new Harness();
    room.joined();
    for (let index = 0; index < 6; index += 1) room.take({ type: 'local', update: update(300) });
    room.take({ type: 'settle' });
    expect(room.sent().length).toBeGreaterThan(1);
  });
});

describe('receiving the document again', () => {
  it('DOC_RESET of a new epoch: the document is another, and changes not taken are said lost', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.take({ type: 'message', message: { action: 'DOC_RESET', doc: 1, epoch: 8 } });
    expect(room.room.status).toMatchObject({ phase: 'receiving', writable: false });
    // The late answer of the change sent in epoch 7 is about the document dropped.
    expect(
      room.take({
        type: 'message',
        message: { action: 'DOC_NACK', doc: 1, epoch: 7, reason: 'update_refused' },
      }),
    ).toEqual([]);
    expect(
      room.take({ type: 'frame', frame: { doc: 1, epoch: 7, sync: UPDATE, payload: WHOLE } }),
    ).toEqual([]);
    room.take({ type: 'frame', frame: { doc: 1, epoch: 8, sync: STEP_2, payload: WHOLE } });
    expect(room.of('discarded')).toEqual([{ type: 'discarded', why: 'document_reset' }]);
    expect(room.of('fresh')).toHaveLength(1);
    expect(room.room.status).toMatchObject({ phase: 'joined', pending: false, writable: true });
  });

  it('DOC_RESET of the same epoch: the whole is merged into the document held, nothing lost', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.take({ type: 'message', message: { action: 'DOC_RESET', doc: 1, epoch: 7 } });
    room.take({ type: 'frame', frame: { doc: 1, epoch: 7, sync: STEP_2, payload: WHOLE } });
    expect(room.effects.map((effect) => effect.type)).toEqual(['apply']);
    room.take({ type: 'message', message: { action: 'DOC_ACK', doc: 1, epoch: 7 } });
    expect(room.room.status.pending).toBe(false);
  });

  it('DOC_NACK: joins once more, drops the document for the server, and says the change is lost', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.write();
    room.take({
      type: 'message',
      message: { action: 'DOC_NACK', doc: 1, epoch: 7, reason: 'update_refused' },
    });
    expect(room.of('join')).toHaveLength(1);
    // The second change's answer comes for a room left: nothing more.
    expect(
      room.take({
        type: 'message',
        message: { action: 'DOC_NACK', doc: 1, epoch: 7, reason: 'update_refused' },
      }),
    ).toEqual([]);
    room.take({
      type: 'message',
      message: { action: 'DOC_JOINED', doc: 2, epoch: 7, mode: 'write' },
    });
    room.take({ type: 'frame', frame: { doc: 2, epoch: 7, sync: STEP_2, payload: WHOLE } });
    expect(room.of('discarded')).toEqual([{ type: 'discarded', why: 'update_refused' }]);
    expect(room.of('fresh')).toHaveLength(1);
  });

  it('DOC_NACK out of step: waits for the DOC_RESET that explains it, then joins if none came', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.take({
      type: 'message',
      message: { action: 'DOC_NACK', doc: 1, epoch: 7, reason: 'update_out_of_step' },
    });
    expect(room.of('timer')).toEqual([{ type: 'timer', timer: 'reset', wait: RESET_WAIT }]);
    expect(room.room.status.writable).toBe(false);
    room.take({ type: 'timer', timer: 'reset' });
    expect(room.of('join')).toHaveLength(1);
  });

  it('DOC_NACK document unavailable: the change is kept and offered again once let in', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.take({
      type: 'message',
      message: { action: 'DOC_NACK', doc: 1, epoch: 7, reason: 'document_unavailable' },
    });
    expect(room.of('timer')).toEqual([{ type: 'timer', timer: 'retry', wait: PACE_WAIT }]);
    room.take({ type: 'timer', timer: 'retry' });
    room.take({
      type: 'message',
      message: { action: 'DOC_JOINED', doc: 2, epoch: 7, mode: 'write' },
    });
    room.take({ type: 'frame', frame: { doc: 2, epoch: 7, sync: STEP_2, payload: WHOLE } });
    expect(room.of('discarded')).toEqual([]);
    expect(room.of('offer')).toEqual([{ type: 'offer', have: WHOLE }]);
    room.take({ type: 'missing', update: update() });
    room.take({ type: 'timer', timer: 'flush' });
    expect(room.sent()).toHaveLength(1);
  });

  it.each([['too_many_frames'], ['connection_too_slow']])(
    'DOC_LEFT for %s: joins again after a pause, going on with the document held',
    (reason) => {
      const room = new Harness();
      room.joined(1, 7);
      room.take({ type: 'message', message: { action: 'DOC_LEFT', doc: 1, reason } });
      expect(room.room.status.phase).toBe('receiving');
      expect(room.of('timer')).toEqual([{ type: 'timer', timer: 'retry', wait: PACE_WAIT }]);
      room.take({ type: 'timer', timer: 'retry' });
      room.take({
        type: 'message',
        message: { action: 'DOC_JOINED', doc: 2, epoch: 7, mode: 'write' },
      });
      room.take({ type: 'frame', frame: { doc: 2, epoch: 7, sync: STEP_2, payload: WHOLE } });
      expect(room.effects.map((effect) => effect.type)).toEqual(['apply', 'changed']);
    },
  );

  it.each([['document_unavailable'], ['too_many_documents'], ['internal']])(
    'DOC_LEFT for %s: out, and tried again by itself',
    (reason) => {
      const room = new Harness();
      room.joined(1, 7);
      room.take({ type: 'message', message: { action: 'DOC_LEFT', doc: 1, reason } });
      expect(room.room.status).toMatchObject({ phase: 'out', reason, retrying: true, held: true });
      room.take({ type: 'timer', timer: 'retry' });
      expect(room.of('join')).toHaveLength(1);
    },
  );

  it('a dropped connection: read only until it is back, then joined again', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.take({ type: 'down' });
    expect(room.room.status).toMatchObject({ phase: 'offline', writable: false, held: true });
    // Nothing can be sent while there is no connection.
    expect(room.write()).toEqual([]);
    room.take({ type: 'up' });
    expect(room.of('join')).toHaveLength(1);
    room.take({
      type: 'message',
      message: { action: 'DOC_JOINED', doc: 2, epoch: 7, mode: 'write' },
    });
    room.take({ type: 'frame', frame: { doc: 2, epoch: 7, sync: STEP_2, payload: WHOLE } });
    // The document held goes on; what was written meanwhile is offered.
    expect(room.of('offer')).toHaveLength(1);
  });

  it('a dropped connection with the document made anew meanwhile: the change written is said lost', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.take({ type: 'down' });
    room.take({ type: 'up' });
    room.take({
      type: 'message',
      message: { action: 'DOC_JOINED', doc: 2, epoch: 9, mode: 'write' },
    });
    room.take({ type: 'frame', frame: { doc: 2, epoch: 9, sync: STEP_2, payload: WHOLE } });
    expect(room.of('discarded')).toEqual([{ type: 'discarded', why: 'document_reset' }]);
    expect(room.of('fresh')).toHaveLength(1);
  });
});

describe('the way of writing', () => {
  it('DOC_MODE read stops the sending; a change made meanwhile is the document no longer being the server, joined again on write', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.take({ type: 'message', message: { action: 'DOC_MODE', doc: 1, mode: 'read' } });
    expect(room.room.status.writable).toBe(false);
    room.take({ type: 'local', update: update() });
    room.take({ type: 'message', message: { action: 'DOC_MODE', doc: 1, mode: 'write' } });
    expect(room.of('join')).toHaveLength(1);
  });

  it('a change refused as read only turns the room to reading and receives the document again', () => {
    const room = new Harness();
    room.joined(1, 7);
    room.write();
    room.take({
      type: 'message',
      message: { action: 'DOC_NACK', doc: 1, epoch: 7, reason: 'document_read_only' },
    });
    expect(room.room.status.mode).toBe('read');
    expect(room.of('join')).toHaveLength(1);
  });
});
