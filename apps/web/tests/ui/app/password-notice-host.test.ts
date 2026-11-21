/**
 * When the notice to set a password stands at the foot of the pane: in the
 * reader's own space, for an account without a password, once something is
 * made there, until it is closed for good, which the account keeps (the
 * client state) so that it does not come back. Against the backend in memory,
 * whose reader is made an account without a password.
 */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { firstKanban } from '$lib/state/workspace-entry.ts';
import type { Account } from '$lib/state/index.ts';

import PasswordNoticeHost from '../../../src/app/PasswordNoticeHost.svelte';
import { setup, settle } from '../../state/support.ts';

afterEach(cleanup);

async function shown({ own = true, hasPassword = false, made = true, kept = {} } = {}) {
  const setUp = setup();
  const { world, backend, workspace } = setUp;
  const reader = world.users.get(backend.userId);
  if (!reader) throw new Error('no reader');
  world.users.set(reader.id, { ...reader, hasPassword });
  world.screenState.set(reader.id, kept);
  const listed = await backend.workspaces();
  const at = listed.find((one) => (one.kind === 'personal') === own);
  if (!at) throw new Error('no such workspace');
  if (made) await backend.createBoard(at.id, { title: 'Mine' }, firstKanban([], 'Board'));
  await workspace.load(at.id);
  const setPassword = vi.fn<Account['setPassword']>(() => Promise.resolve());
  const account = { setPassword, loginOptions: vi.fn() } as Partial<Account> as Account;
  const onProvider = vi.fn(() => Promise.resolve());
  render(PasswordNoticeHost, { props: { workspace, account, onProvider } });
  await settle();
  return { ...setUp, setPassword, readerId: reader.id };
}

describe('the notice to set a password, in the app', () => {
  it('stands in the own space of an account without a password once something is in it', async () => {
    await shown();
    expect(await screen.findByTestId('password-notice')).toBeInTheDocument();
  });

  it('is not there before anything is made, in a shared workspace, or with a password', async () => {
    for (const made of [
      { made: false },
      { own: false },
      { hasPassword: true },
      { kept: { passwordNoticeDismissed: '1' } },
    ]) {
      await shown(made);
      await settle();
      expect(screen.queryByTestId('password-notice')).toBeNull();
      cleanup();
    }
  });

  it('comes when the first thing is made, and is closed for good with the account', async () => {
    const { backend, workspace, world, readerId } = await shown({ made: false });
    expect(screen.queryByTestId('password-notice')).toBeNull();
    await workspace.createBoard({ title: 'First' }, 'Board', 'Made');
    expect(await screen.findByTestId('password-notice')).toBeInTheDocument();
    await fireEvent.click(screen.getByTestId('password-notice-dismiss'));
    expect(screen.queryByTestId('password-notice')).toBeNull();
    await waitFor(async () => {
      expect((await backend.screenState()).passwordNoticeDismissed).toBe('1');
    });
    expect(world.screenState.get(readerId)).toMatchObject({ passwordNoticeDismissed: '1' });
  });

  it('sets the password through the account, and then the own space asks for it', async () => {
    const { setPassword, workspace } = await shown();
    await fireEvent.input(await screen.findByLabelText('New password'), {
      target: { value: 'a long password' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Set password' }));
    await waitFor(() => {
      expect(setPassword).toHaveBeenCalledExactlyOnceWith('a long password');
    });
    // Set, the account has one: the notice is gone, and the space is closed to this session.
    await waitFor(() => {
      expect(screen.queryByTestId('password-notice')).toBeNull();
    });
    expect(workspace.me?.hasPassword).toBe(true);
    expect(workspace.entered).toBe('password');
  });

  it('asks nothing again once set, wherever the reader goes and comes back', async () => {
    const { workspace, world } = await shown();
    await fireEvent.input(await screen.findByLabelText('New password'), {
      target: { value: 'a long password' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Set password' }));
    await waitFor(() => {
      expect(workspace.entered).toBe('password');
    });
    const own = workspace.closed?.id ?? '';
    await workspace.enter(world.workspace.id);
    expect(workspace.entered).toBe('ready');
    await workspace.enter(own);
    await settle();
    expect(workspace.entered).toBe('password');
    expect(screen.queryByTestId('password-notice')).toBeNull();
  });
});
