import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import ActiveTimerPill from '$lib/features/time/ActiveTimerPill.svelte';
import MyTimeScreen from '$lib/features/time/MyTimeScreen.svelte';
import TargetTimerButton from '$lib/features/time/TargetTimerButton.svelte';
import { timeTracker, Workspace, type TimeEntry } from '$lib/state/index.ts';
import { MemoryBackend } from '$lib/state/memory/backend.ts';
import { seedWorld } from '$lib/state/memory/seed.ts';

const SAMPLE_ENTRY: TimeEntry = {
  id: '01930000-0000-7000-8000-000000000001',
  workspaceId: 'ws-1',
  containerId: 'b-1',
  blockId: 'c-1',
  targetKind: 'card',
  targetTitle: 'Linear Algebra Homework',
  kind: 'study',
  startedAt: 1_700_000_000_000,
  endedAt: null,
  note: '',
  createAt: 1_700_000_000_000,
  updateAt: 1_700_000_000_000,
};

afterEach(() => {
  document.body.replaceChildren();
});

describe('ActiveTimerPill', () => {
  it('renders target title, kind badge, HH:MM:SS clock, and triggers open/stop callbacks', async () => {
    const onOpen = vi.fn();
    const onStop = vi.fn();
    render(ActiveTimerPill, {
      props: {
        entry: SAMPLE_ENTRY,
        elapsedMs: 3_723_000,
        busy: false,
        href: '/w/ws-1/boards/b-1/-/c-1',
        onOpen,
        onStop,
      },
    });

    expect(screen.getByTestId('active-timer-title').textContent).toContain(
      'Linear Algebra Homework',
    );
    expect(screen.getByTestId('active-timer-elapsed').textContent.trim()).toBe('01:02:03');

    await fireEvent.click(screen.getByTestId('active-timer-link'));
    expect(onOpen).toHaveBeenCalledOnce();

    await fireEvent.click(screen.getByTestId('active-timer-stop'));
    expect(onStop).toHaveBeenCalledOnce();
  });
});

describe('TargetTimerButton', () => {
  it('starts with default kind when idle and stops when running', async () => {
    const onStart = vi.fn();
    const onStop = vi.fn();
    const { rerender } = render(TargetTimerButton, {
      props: {
        running: false,
        elapsedMs: 0,
        busy: false,
        defaultKind: 'lecture',
        onStart,
        onStop,
      },
    });

    await fireEvent.click(screen.getByTestId('target-timer-start'));
    expect(onStart).toHaveBeenCalledWith('lecture');

    await rerender({
      running: true,
      elapsedMs: 65_000,
      busy: false,
      defaultKind: 'lecture',
      onStart,
      onStop,
    });
    expect(screen.getByTestId('target-timer-elapsed').textContent.trim()).toBe('00:01:05');
    await fireEvent.click(screen.getByTestId('target-timer-stop'));
    expect(onStop).toHaveBeenCalledOnce();
  });
});

describe('MyTimeScreen', () => {
  it('renders entries, switches periods, filters by kind, and validates manual entry end < start', async () => {
    const { world, readerId } = seedWorld([]);
    const backend = new MemoryBackend(world, readerId);
    const workspace = new Workspace({ backend, locale: () => 'en' });
    await workspace.load(world.workspace.id);
    const now = Date.now();
    await backend.time.createTimeEntry({
      kind: 'study',
      startedAt: now - 3_600_000,
      endedAt: now - 1_800_000,
      note: 'Review notes',
    });
    const tracker = timeTracker(workspace);
    if (!tracker) throw new Error('expected tracker');

    render(MyTimeScreen, {
      props: {
        tracker,
        locale: 'en',
        targetHref: () => undefined,
        onOpenTarget: () => undefined,
      },
    });

    await waitFor(() => {
      expect(screen.getByTestId('time-entry-row')).toBeTruthy();
    });
    expect(screen.getByTestId('time-summary-total').textContent).toContain('30m');
    expect(screen.getByTestId('time-entry-note').textContent).toBe('Review notes');

    // Switch to lecture filter -> empty state
    await fireEvent.click(screen.getByTestId('time-filter-lecture'));
    await waitFor(() => {
      expect(screen.getByTestId('time-empty-state')).toBeTruthy();
    });

    // Switch back to all and switch period to week
    await fireEvent.click(screen.getByTestId('time-filter-all'));
    await fireEvent.click(screen.getByTestId('time-period-week'));
    await waitFor(() => {
      expect(screen.getByTestId('time-entry-row')).toBeTruthy();
    });

    // Open manual add dialog and test endedAt < startedAt inline validation
    await fireEvent.click(screen.getByTestId('time-add-button'));
    const startInput = await screen.findByTestId('time-form-started-at');
    const endInput = screen.getByTestId('time-form-ended-at');
    await fireEvent.input(startInput, { target: { value: '2026-11-24T12:00' } });
    await fireEvent.input(endInput, { target: { value: '2026-11-24T11:00' } });
    await fireEvent.click(screen.getByTestId('time-form-submit'));
    await waitFor(() => {
      expect(screen.getByTestId('time-form-error').textContent).not.toBe('');
    });

    // Editing only note on a sub-minute entry with non-zero seconds omits unchanged timestamps
    await fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    const preciseStart = now - 45_678;
    const preciseEnd = now - 12_345;
    const precise = await backend.time.createTimeEntry({
      kind: 'work',
      startedAt: preciseStart,
      endedAt: preciseEnd,
      note: 'Original',
    });
    await tracker.refreshAfterUnlock();
    const editButtons = await screen.findAllByTestId('time-entry-edit');
    const firstEdit = editButtons[0];
    if (!firstEdit) throw new Error('expected edit button');
    await fireEvent.click(firstEdit);
    const noteInput = await screen.findByTestId('time-form-note');
    await fireEvent.input(noteInput, { target: { value: 'Updated note only' } });
    await fireEvent.click(screen.getByTestId('time-form-submit'));
    await waitFor(async () => {
      const listed = await backend.time.listTimeEntries();
      const updated = listed.entries.find((item) => item.id === precise.id);
      expect(updated?.note).toBe('Updated note only');
      expect(updated?.startedAt).toBe(preciseStart);
      expect(updated?.endedAt).toBe(preciseEnd);
    });

    workspace.close();
  });
});
