/**
 * The people of a workspace in their dialog: what its administrator sees and
 * does there, against a backend that answers as the server would; and what
 * the sidebar leaves out for a guest.
 */
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import PeoplePanel from '$lib/features/shell/PeoplePanel.svelte';
import {
  BackendError,
  type Invitation,
  type People,
  type Workspace,
  type WorkspaceMember,
} from '$lib/state/index.ts';

afterEach(cleanup);

const MEMBERS: WorkspaceMember[] = [
  { userId: 'u1', username: 'ada', displayName: 'Ada', role: 'admin' },
  { userId: 'u2', username: 'grace', displayName: '', role: 'member' },
  { userId: 'u3', username: 'visitor', displayName: '', role: 'guest' },
];

function link(id: string, role: 'member' | 'guest' = 'member'): Invitation {
  return { id, role, createdBy: 'u1', createAt: 0, expiresAt: 7 * 86_400_000, uses: 2 };
}

/** The requests the panel makes, each a mock that answers as the server would. */
function requests() {
  return {
    members: vi.fn<People['members']>(() => Promise.resolve(MEMBERS)),
    setRole: vi.fn<People['setRole']>((_workspace, userId, role) =>
      Promise.resolve({ userId, username: 'grace', displayName: '', role }),
    ),
    removeMember: vi.fn<People['removeMember']>(() => Promise.resolve()),
    invitations: vi.fn<People['invitations']>(() =>
      Promise.resolve([link('l1'), link('l2', 'guest')]),
    ),
    inviteMembers: vi.fn<People['inviteMembers']>(() =>
      Promise.resolve({ ...link('l3'), token: 'the-token' }),
    ),
    inviteGuest: vi.fn<People['inviteGuest']>(() => Promise.reject(new Error('not here'))),
    revokeInvitation: vi.fn<People['revokeInvitation']>(() => Promise.resolve()),
    guestInvitations: vi.fn<People['guestInvitations']>(() => Promise.resolve(true)),
    setGuestInvitations: vi.fn<People['setGuestInvitations']>((_workspace, enabled) =>
      Promise.resolve(enabled),
    ),
  };
}

function mount(overrides: Partial<ReturnType<typeof requests>> = {}) {
  const people = { ...requests(), ...overrides };
  // What the panel reads of the workspace store, and no more.
  const workspace = {
    info: { id: 'w1', title: 'Acme', role: 'admin' },
    me: { id: 'u1' },
    locale: 'en',
    regional: { timeZone: 'UTC', hourCycle: 'h23' },
  } as unknown as Workspace;
  render(PeoplePanel, {
    props: { workspace, people, inviteLink: (token: string) => `https://here/invite/${token}` },
  });
  return people;
}

describe('the people of a workspace', () => {
  it('lists everybody with what they are, and offers each the roles a change can make', async () => {
    mount();
    const list = await screen.findByTestId('people-members');
    const rows = within(list).getAllByRole('listitem');
    expect(rows.map((row) => row.textContent.includes('Ada (@ada)'))).toEqual([true, false, false]);
    const options = (name: string): string[] =>
      within(screen.getByLabelText(`Role of ${name}`))
        .getAllByRole('option')
        .map((option) => option.textContent);
    expect(options('Ada (@ada)')).toEqual(['Admin', 'Member']);
    expect(options('grace')).toEqual(['Admin', 'Member', 'Guest']);
    // A guest becomes a member by a link, not here.
    expect(screen.getByLabelText('Role of visitor')).toBeDisabled();
    // Nobody takes themselves out.
    expect(screen.getByRole('button', { name: 'Remove Ada (@ada)' })).toBeDisabled();
    // The links for members are listed; one for a guest is its board's.
    expect(within(screen.getByTestId('people-links')).getAllByRole('listitem')).toHaveLength(1);
  });

  it('shows the address of a link once it is made, and revokes one', async () => {
    const people = mount();
    await fireEvent.click(await screen.findByTestId('people-make-link'));
    expect(await screen.findByTestId('people-link')).toHaveValue('https://here/invite/the-token');
    expect(people.inviteMembers).toHaveBeenCalledExactlyOnceWith('w1');
    // The lists are read again after the link is made; then the row can be used.
    const revoke = screen.getByRole('button', { name: 'Revoke' });
    await waitFor(() => {
      expect(revoke).toBeEnabled();
    });
    await fireEvent.click(revoke);
    await waitFor(() => {
      expect(people.revokeInvitation).toHaveBeenCalledExactlyOnceWith('w1', 'l1');
    });
    await waitFor(() => {
      expect(screen.queryByTestId('people-link')).not.toBeInTheDocument();
    });
  });

  it('changes a role, says why the last administrator stays one, and reads again', async () => {
    const people = mount({
      setRole: vi.fn<People['setRole']>(() =>
        Promise.reject(new BackendError(409, { detail: 'no', reason: 'last_admin' })),
      ),
    });
    const role = await screen.findByLabelText('Role of Ada (@ada)');
    await fireEvent.change(role, { target: { value: 'member' } });
    expect(await screen.findByTestId('people-problem')).toHaveTextContent(
      /needs an admin. Make somebody else one first./,
    );
    expect(people.setRole).toHaveBeenCalledExactlyOnceWith('w1', 'u1', 'member');
    expect(people.members).toHaveBeenCalledTimes(2);
  });

  it('switches whether guests are invited', async () => {
    const people = mount();
    await fireEvent.click(await screen.findByTestId('people-guests'));
    await waitFor(() => {
      expect(people.setGuestInvitations).toHaveBeenCalledExactlyOnceWith('w1', false);
    });
  });

  it('says so when the people cannot be read, and tries again', async () => {
    const members = vi
      .fn<People['members']>()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue(MEMBERS);
    mount({ members });
    await fireEvent.click(await screen.findByRole('button', { name: 'Try again' }));
    expect(await screen.findByTestId('people-members')).toBeInTheDocument();
  });
});
