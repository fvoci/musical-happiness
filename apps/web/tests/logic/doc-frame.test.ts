/**
 * The binary frame of a room (`logic/doc-frame.ts`) against the contract's
 * example (`contracts/websocket.md`, section 6), and what is dropped.
 */
import { describe, expect, it } from 'vitest';
import * as Y from 'yjs';

import {
  decodeDocFrame,
  encodeDocFrame,
  SYNC_STEP_1,
  SYNC_STEP_2,
  SYNC_UPDATE,
} from '$lib/logic/doc-frame.ts';

const hex = (bytes: Uint8Array): string =>
  [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join(' ');

describe('a frame of a room', () => {
  it('is the example of the contract: doc 3, epoch 300, an update of aa bb', () => {
    const frame = { doc: 3, epoch: 300, sync: SYNC_UPDATE, payload: new Uint8Array([0xaa, 0xbb]) };
    const bytes = encodeDocFrame(frame);
    expect(hex(bytes)).toBe('03 00 ac 02 02 02 aa bb');
    expect(decodeDocFrame(bytes)).toEqual(frame);
  });

  it('writes large numbers as varuints of lib0, seven bits a byte from the low end', () => {
    const doc = new Y.Doc();
    doc.getText('t').insert(0, 'hello');
    for (const [sync, payload] of [
      [SYNC_STEP_1, Y.encodeStateVector(doc)],
      [SYNC_STEP_2, Y.encodeStateAsUpdate(doc)],
    ] as const) {
      const frame = { doc: 70_000, epoch: 2 ** 40, sync, payload };
      const bytes = encodeDocFrame(frame);
      const head = `f0 a2 04 00 80 80 80 80 80 20 0${String(sync)} ${hex(new Uint8Array([payload.length]))}`;
      expect(hex(bytes)).toBe(`${head} ${hex(payload)}`);
      expect(decodeDocFrame(bytes)).toEqual(frame);
    }
  });

  it('is dropped when it is no sync message, or its bytes are not what it says', () => {
    const good = encodeDocFrame({
      doc: 1,
      epoch: 1,
      sync: SYNC_UPDATE,
      payload: new Uint8Array([1]),
    });
    // Awareness (kind 1) is not read yet.
    expect(decodeDocFrame(new Uint8Array([1, 1, 1, 0, 1, 1]))).toBeUndefined();
    // A sync message of no known kind.
    expect(decodeDocFrame(new Uint8Array([1, 0, 1, 3, 1, 1]))).toBeUndefined();
    // Shorter and longer than its length.
    expect(decodeDocFrame(good.subarray(0, good.length - 1))).toBeUndefined();
    expect(decodeDocFrame(new Uint8Array([...good, 0]))).toBeUndefined();
    // A number that does not end, and one too large to count with.
    expect(decodeDocFrame(new Uint8Array([0x80, 0x80]))).toBeUndefined();
    expect(
      decodeDocFrame(new Uint8Array([...new Array<number>(9).fill(0xff), 0x7f])),
    ).toBeUndefined();
    expect(decodeDocFrame(new Uint8Array())).toBeUndefined();
  });
});
