/**
 * The guest links of a shared board in its share dialog: the ones mailed to
 * somebody by their address, each to be revoked, against a backend that answers
 * as the server would.
 */
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import GuestInvite from '$lib/features/shell/GuestInvite.svelte';
import { BackendError, type Invitation, type People, type Workspace } from '$lib/state/index.ts';

afterEach(cleanup);

function link(id: string, extra: Partial<Invitation> = {}): Invitation {
  return {
    id,
    role: 'guest',
    boardId: 'b1',
    createdBy: 'u1',
    createAt: 0,
    expiresAt: 7 * 86_400_000,
    uses: 0,
    ...extra,
  };
}

const LINKS: Invitation[] = [
  link('parent'),
  link('child', { parentId: 'parent', recipient: 'guest@example.test' }),
  link('other-board', { boardId: 'b2', recipient: 'elsewhere@example.test' }),
  { ...link('members'), role: 'member' },
];

function mount(read: People['invitations']) {
  const revokeInvitation = vi.fn<People['revokeInvitation']>(() => Promise.resolve());
  const invitations = vi.fn(read);
  const people = {
    invitations,
    revokeInvitation,
    inviteGuest: vi.fn<People['inviteGuest']>(() => Promise.reject(new Error('not here'))),
  } as unknown as People;
  const workspace = {
    info: { id: 'w1', title: 'Acme', role: 'admin' },
    locale: 'en',
    regional: { timeZone: 'UTC', hourCycle: 'h23' },
  } as unknown as Workspace;
  render(GuestInvite, {
    props: { workspace, access: { people, inviteLink: (token: string) => token }, boardId: 'b1' },
  });
  return { invitations, revokeInvitation };
}

describe('the guest links of a shared board', () => {
  it('lists the board’s guest links, the mailed ones by their address, and revokes one', async () => {
    const { revokeInvitation } = mount(() => Promise.resolve(LINKS));
    const list = await screen.findByTestId('share-guest-links');
    const rows = within(list).getAllByRole('listitem');
    expect(rows.map((row) => row.textContent.replace(/\s+/g, ' ').trim())).toEqual([
      'Link · ends January 08, 1970 Revoke link',
      'Mailed to guest@example.test · ends January 08, 1970 Revoke link',
    ]);
    const mailed = rows[1];
    if (!mailed) throw new Error('two rows');
    await fireEvent.click(within(mailed).getByRole('button', { name: 'Revoke link' }));
    await waitFor(() => {
      expect(revokeInvitation).toHaveBeenCalledWith('w1', 'child');
    });
  });

  it('leaves the list out when it cannot be read', async () => {
    const { invitations } = mount(() => Promise.reject(new BackendError(403, { detail: 'no' })));
    await waitFor(() => {
      expect(invitations).toHaveBeenCalled();
    });
    expect(screen.queryByTestId('share-guest-links')).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
