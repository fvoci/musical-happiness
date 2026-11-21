import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { BackendError } from '$lib/state/index.ts';
import {
  failureOf,
  TRANSFER_STAYS,
  Transfers,
  type TransferOptions,
} from '$lib/state/transfers.svelte.ts';

/** A send the test ends by hand: how far it went, and how it ends. */
function held_<T>() {
  let options: TransferOptions | undefined;
  let finish: (value: T) => void = () => undefined;
  let fail: (error: unknown) => void = () => undefined;
  const send = (given: TransferOptions): Promise<T> => {
    options = given;
    return new Promise<T>((resolve, reject) => {
      finish = resolve;
      fail = reject;
    });
  };
  return {
    send,
    get options() {
      if (!options) throw new Error('not sent');
      return options;
    },
    finish: (value: T) => {
      finish(value);
    },
    fail: (error: unknown) => {
      fail(error);
    },
  };
}

const file = (name: string, size = 10): File => new File(['x'.repeat(size)], name);

function made() {
  const ends = vi.fn<(error: unknown) => boolean>(() => false);
  const report = vi.fn();
  const shell = { report, ends, info: { id: 'one' } as { id: string } | undefined };
  return { transfers: new Transfers(shell), ends, report, shell };
}

describe('uploads in the stack', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('shows how far one is, says only its start and its end, and lets it go a moment after', async () => {
    const { transfers } = made();
    const sending = held_<string>();
    const { done } = transfers.upload({
      file: file('a.txt'),
      send: sending.send,
    });
    expect(transfers.list).toMatchObject([{ kind: 'upload', name: 'a.txt', done: 0, total: 10 }]);
    expect(transfers.said).toMatchObject({ name: 'a.txt', finished: false });
    const started = transfers.said;
    sending.options.onProgress({ sentBytes: 4, totalBytes: 10 });
    expect(transfers.list[0]).toMatchObject({ done: 4, total: 10 });
    // A count that changes is not said.
    expect(transfers.said).toBe(started);
    sending.finish('file-1');
    expect(await done).toBe('file-1');
    expect(transfers.said).toMatchObject({ name: 'a.txt', finished: true });
    vi.advanceTimersByTime(TRANSFER_STAYS - 1);
    expect(transfers.list).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(transfers.list).toEqual([]);
  });

  it('keeps the finished while it is held, and gives them their whole moment once let go', async () => {
    const { transfers } = made();
    let held = false;
    const stop = transfers.holdWhile(() => held);
    const sending = held_<string>();
    const { done } = transfers.upload({ file: file('a.txt'), send: sending.send });
    sending.finish('file-1');
    await done;
    held = true;
    vi.advanceTimersByTime(TRANSFER_STAYS * 3);
    expect(transfers.list).toHaveLength(1);
    held = false;
    transfers.restart();
    vi.advanceTimersByTime(TRANSFER_STAYS - 1);
    expect(transfers.list).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(transfers.list).toEqual([]);
    // No more asked once stopped.
    held = true;
    stop();
    const again = held_<string>();
    const next = transfers.upload({ file: file('b.txt'), send: again.send });
    again.finish('file-2');
    await next.done;
    vi.advanceTimersByTime(TRANSFER_STAYS);
    expect(transfers.list).toEqual([]);
  });

  it('keeps a failed one with why until it is dismissed, and sends it again where it may', async () => {
    const { transfers, ends, report } = made();
    let tries = 0;
    const send = (): Promise<string> => {
      tries += 1;
      return tries === 1
        ? Promise.reject(new BackendError(429, { reason: 'too_many_uploads' }))
        : Promise.resolve('file-2');
    };
    const { done } = transfers.upload({
      file: file('b.bin'),
      send,
      retryable: true,
    });
    expect(await done).toBeUndefined();
    expect(transfers.list).toMatchObject([{ failure: 'busy', finished: false }]);
    expect(transfers.said).toMatchObject({ failure: 'busy' });
    expect(ends).toHaveBeenCalledOnce();
    // Told in the stack only.
    expect(report).not.toHaveBeenCalled();
    vi.advanceTimersByTime(TRANSFER_STAYS * 10);
    expect(transfers.list).toHaveLength(1);
    transfers.list[0]?.retry?.();
    expect(transfers.list[0]).toMatchObject({ failure: undefined, retry: undefined, done: 0 });
    await vi.waitFor(() => {
      expect(transfers.list[0]).toMatchObject({ finished: true });
    });
    expect(tries).toBe(2);
  });

  it('has no way to send again one that cannot be', async () => {
    const { transfers } = made();
    const send = (): Promise<string> => Promise.reject(new BackendError(0, { detail: 'gone' }));
    await transfers.upload({ file: file('c.png'), send }).done;
    expect(transfers.list).toMatchObject([{ failure: 'network' }]);
    expect(transfers.list[0]?.retry).toBeUndefined();
  });

  it('offers to send again only what sending again may get past', async () => {
    const { transfers } = made();
    const failing = (error: BackendError) => (): Promise<string> => Promise.reject(error);
    const tried = [
      [new BackendError(413, { limit: 2048, reason: 'file_too_large' }), false],
      [new BackendError(400, { detail: 'name' }), false],
      [new BackendError(403, { detail: 'not yours' }), false],
      [new BackendError(0, { detail: 'gone' }), true],
      [new BackendError(429, { reason: 'uploads_pending' }), true],
      [new BackendError(500, { detail: 'broken' }), true],
    ] as const;
    for (const [error] of tried) {
      await transfers.upload({ file: file('e.bin'), send: failing(error), retryable: true }).done;
    }
    expect(transfers.list.map((one) => one.retry !== undefined)).toEqual(
      tried.map(([, offered]) => offered),
    );
  });

  it('gives up one that is dismissed under way, and tells nothing of it', async () => {
    const { transfers, ends } = made();
    const sending = held_<string>();
    const { id, done } = transfers.upload({
      file: file('d.txt'),
      send: sending.send,
    });
    const said = transfers.said;
    transfers.dismiss(id);
    expect(sending.options.signal.aborted).toBe(true);
    sending.fail(new BackendError(0, { detail: 'given up' }));
    expect(await done).toBeUndefined();
    expect(transfers.list).toEqual([]);
    expect(transfers.said).toBe(said);
    expect(ends).not.toHaveBeenCalled();
  });

  it('marks one left behind by what it was going up for, and keeps it until dismissed', async () => {
    const { transfers } = made();
    const sending = held_<string>();
    const { id, done } = transfers.upload({
      file: file('e.png'),
      send: sending.send,
    });
    transfers.leave(id);
    expect(sending.options.signal.aborted).toBe(true);
    sending.fail(new BackendError(0, { detail: 'given up' }));
    expect(await done).toBeUndefined();
    expect(transfers.list).toMatchObject([{ failure: 'left' }]);
    expect(transfers.said).toMatchObject({ failure: 'left' });
    vi.advanceTimersByTime(TRANSFER_STAYS * 10);
    expect(transfers.list).toHaveLength(1);
  });

  it('shows each in the workspace it was started in only', () => {
    const { transfers, shell } = made();
    const never = (): Promise<string> => new Promise(() => undefined);
    transfers.upload({ file: file('here.txt'), send: never });
    shell.info = { id: 'two' };
    transfers.upload({ file: file('there.txt'), send: never });
    expect(transfers.of('one').map((one) => one.name)).toEqual(['here.txt']);
    expect(transfers.of('two').map((one) => one.name)).toEqual(['there.txt']);
    expect(transfers.of(undefined)).toEqual([]);
  });
});

describe('why an upload failed', () => {
  it.each([
    [new BackendError(0, { detail: 'no answer' }), { failure: 'network' }],
    [new BackendError(400, { detail: 'bad name' }), { failure: 'refused' }],
    [new BackendError(403, { detail: 'no' }), { failure: 'forbidden' }],
    [new BackendError(404, { detail: 'gone' }), { failure: 'forbidden' }],
    [new BackendError(413, { limit: 5 }), { failure: 'too-large', limit: 5 }],
    [new BackendError(413, { detail: 'big' }), { failure: 'too-large' }],
    [new BackendError(429, { reason: 'too_many_uploads' }), { failure: 'busy' }],
    [new BackendError(429, { reason: 'uploads_pending' }), { failure: 'room' }],
    [new BackendError(429, { reason: 'server_uploads_pending' }), { failure: 'room' }],
    [new BackendError(500, { detail: 'oops' }), { failure: 'failed' }],
    [new Error('a bug'), { failure: 'failed' }],
  ])('%s', (error, expected) => {
    expect(failureOf(error)).toEqual(expected);
  });
});
