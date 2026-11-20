/**
 * The lease of a file open in the editor, as the app keeps it
 * (lib/file-viewer/lease.svelte.ts): taken by "Edit" and given back where a
 * newer version came meanwhile; held on to every while and when the window
 * is shown again; let go of when the editor closes and as the page goes; a
 * lease lost told. And who edits the file, as the viewer says it
 * (EditingNote.svelte): the reader in another window, somebody named, or
 * somebody else.
 */
import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import EditingNote from '$lib/components/file-viewer/EditingNote.svelte';
import {
  EditLease,
  LeaseWatch,
  startEditing,
  type FileLease,
  type HeldFileLease,
  type LeaseSource,
} from '$lib/file-viewer/lease.svelte.ts';

const TIMES = { holdEvery: 1_000, watchEvery: 500 };

/** A file's lease as the server keeps it, for one reader, and what was asked of it. */
function sourceOf(me = 'me') {
  const asked: string[] = [];
  const state = {
    lease: { editing: false, editorId: '' } as FileLease,
    /** What the next holds answer: a lease, another's, or no answer. */
    holding: 'held' as 'held' | 'editing' | 'unreached',
    taken: 0,
    learned: [] as string[],
  };
  const source: LeaseSource = {
    me,
    lease: () => Promise.resolve(state.lease),
    holdLease: (leaseId): Promise<HeldFileLease | 'editing'> => {
      asked.push(`hold ${leaseId}`);
      if (state.holding === 'unreached') return Promise.reject(new Error('unreached'));
      if (state.holding === 'editing') return Promise.resolve('editing');
      if (leaseId === '') state.taken += 1;
      return Promise.resolve({ leaseId: leaseId || `l${String(state.taken)}`, seconds: 90 });
    },
    releaseLease: (leaseId, keepalive) => {
      asked.push(`release ${leaseId}${keepalive ? ' keepalive' : ''}`);
      return Promise.resolve();
    },
    learnUsers: (ids) => {
      state.learned.push(...ids);
      return Promise.resolve();
    },
  };
  return { source, asked, state };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('the lease of a file open in the editor', () => {
  it('is held on to every while and as the window is shown again, and let go of on close', async () => {
    const { source, asked } = sourceOf();
    const lease = new EditLease(source, TIMES);
    expect(await lease.take()).toBe('held');
    await vi.advanceTimersByTimeAsync(2_500);
    expect(asked).toEqual(['hold ', 'hold l1', 'hold l1']);
    document.dispatchEvent(new Event('visibilitychange'));
    await vi.advanceTimersByTimeAsync(0);
    expect(asked.at(-1)).toBe('hold l1');
    await lease.release();
    expect(asked.at(-1)).toBe('release l1');
    const before = asked.length;
    await vi.advanceTimersByTimeAsync(5_000);
    document.dispatchEvent(new Event('visibilitychange'));
    await lease.release();
    expect(asked).toHaveLength(before);
  });

  it('goes with the page, by a request that outlives it', async () => {
    const { source, asked } = sourceOf();
    const lease = new EditLease(source, TIMES);
    await lease.take();
    window.dispatchEvent(new Event('pagehide'));
    await vi.advanceTimersByTimeAsync(0);
    expect(asked.at(-1)).toBe('release l1 keepalive');
    await lease.release();
    expect(asked.filter((one) => one.startsWith('release'))).toHaveLength(1);
  });

  it('says so when another took the file’s meanwhile, and not while it is only unreached', async () => {
    const { source, state } = sourceOf();
    const lease = new EditLease(source, TIMES);
    await lease.take();
    state.holding = 'unreached';
    await vi.advanceTimersByTimeAsync(1_000);
    expect(lease.lost).toBe(false);
    state.holding = 'editing';
    await vi.advanceTimersByTimeAsync(1_000);
    expect(lease.lost).toBe(true);
    // Nobody holds it any more: it is the editor's again.
    state.holding = 'held';
    await vi.advanceTimersByTimeAsync(1_000);
    expect(lease.lost).toBe(false);
    await lease.release();
  });

  it('is not taken while another edits, nor where the server is not reached', async () => {
    const { source, state, asked } = sourceOf();
    state.holding = 'editing';
    expect(await new EditLease(source, TIMES).take()).toBe('editing');
    state.holding = 'unreached';
    expect(await new EditLease(source, TIMES).take()).toBe('failed');
    await vi.advanceTimersByTimeAsync(5_000);
    expect(asked).toEqual(['hold ', 'hold ']);
  });
});

describe('"Edit"', () => {
  it('holds the lease where the version shown is the newest', async () => {
    const { source, asked } = sourceOf();
    const started = await startEditing(source, 3, () => Promise.resolve(3));
    expect(started.as).toBe('held');
    if (started.as === 'held') await started.lease.release();
    expect(asked).toEqual(['hold ', 'release l1']);
  });

  it('gives the lease back where a newer version came meanwhile', async () => {
    const { source, asked } = sourceOf();
    expect(await startEditing(source, 3, () => Promise.resolve(4))).toEqual({ as: 'newer' });
    await vi.advanceTimersByTimeAsync(0);
    expect(asked).toEqual(['hold ', 'release l1']);
  });

  it('opens nothing while another edits the file', async () => {
    const { source, state } = sourceOf();
    state.holding = 'editing';
    const newest = vi.fn(() => Promise.resolve(3));
    expect(await startEditing(source, 3, newest)).toEqual({ as: 'editing' });
    expect(newest).not.toHaveBeenCalled();
  });
});

describe('who edits the file, as the viewer says it', () => {
  const said = () => screen.getByTestId('file-viewer-editing');

  async function shown(lease: FileLease, note?: 'failed' | 'newer') {
    const { source, state } = sourceOf('me');
    state.lease = lease;
    const watch = new LeaseWatch(source, TIMES.watchEvery);
    await watch.read();
    render(EditingNote, {
      watch,
      note,
      userName: (userId: string) => (userId === 'ann' ? 'Ann' : undefined),
    });
    return { watch, state };
  }

  it('names who edits, where the reader may know who', async () => {
    const { state } = await shown({ editing: true, editorId: 'ann' });
    expect(said().textContent.trim()).toBe('Ann is editing this file.');
    expect(said().dataset.editing).toBe('true');
    expect(said().getAttribute('role')).toBe('status');
    expect(state.learned).toEqual(['ann']);
  });

  it('says somebody else where the reader may not know who, and the reader’s other window', async () => {
    await shown({ editing: true, editorId: '' });
    expect(said().textContent.trim()).toBe('Someone else is editing this file.');
    cleanup();
    await shown({ editing: true, editorId: 'me' });
    expect(said().textContent.trim()).toBe('You are editing this file in another window.');
  });

  it('says nothing while nobody edits, and what came of the last "Edit" otherwise', async () => {
    await shown({ editing: false, editorId: '' });
    expect(said().textContent.trim()).toBe('');
    expect(said().dataset.editing).toBe('false');
    cleanup();
    await shown({ editing: false, editorId: '' }, 'newer');
    expect(said().textContent).toContain('A newer version was saved in the meantime.');
  });

  it('asks again every while the page is shown, once the reader’s own lease went', async () => {
    const { source, state } = sourceOf();
    const watch = new LeaseWatch(source, TIMES.watchEvery);
    let release: () => void = () => undefined;
    const released = new Promise<void>((resolve) => {
      release = resolve;
    });
    state.lease = { editing: true, editorId: 'me' };
    const stop = watch.watch(released);
    await vi.advanceTimersByTimeAsync(0);
    expect(watch.lease).toBeUndefined();
    state.lease = { editing: false, editorId: '' };
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(watch.editing).toBe(false);
    state.lease = { editing: true, editorId: '' };
    await vi.advanceTimersByTimeAsync(500);
    expect(watch.editor).toEqual({ who: 'someone' });
    stop();
    state.lease = { editing: false, editorId: '' };
    await vi.advanceTimersByTimeAsync(5_000);
    expect(watch.editing).toBe(true);
  });
});
