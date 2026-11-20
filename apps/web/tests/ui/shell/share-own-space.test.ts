/**
 * The share dialog of a board in the reader's own space, where the backend
 * has people to manage (as the server has): the public link alone, without
 * the workspace's openness, guests, members or teams, none of whose requests
 * the own space answers. In a shared workspace the same dialog has them all.
 */
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import ShareDialog from '$lib/features/shell/ShareDialog.svelte';
import { firstKanban } from '$lib/state/workspace-entry.ts';
import { providePeople, type People, type Shareable } from '$lib/state/index.ts';
import type { Teams } from '$lib/state/team-types.ts';

import { setup } from '../../state/support.ts';

afterEach(cleanup);

async function dialogIn(own: boolean) {
  const made = setup();
  const workspaces = await made.backend.workspaces();
  const shown = workspaces.find((one) => (one.kind === 'personal') === own);
  if (!shown) throw new Error('no such workspace');
  await made.workspace.load(shown.id);
  const invitations = vi.fn<People['invitations']>(() => Promise.resolve([]));
  const list = vi.fn<Teams['list']>(() => Promise.resolve([]));
  const onContainer = vi.fn<Teams['onContainer']>(() => Promise.resolve([]));
  const teams = { list, onContainer } as Partial<Teams> as Teams;
  const people = { invitations, teams } as Partial<People> as People;
  providePeople(made.workspace, { people, inviteLink: (token: string) => token });
  const { board } = await made.backend.createBoard(
    shown.id,
    { title: 'Mine' },
    firstKanban([], 'Board'),
  );
  const members = await made.backend.members(board.id);
  const shareable = {
    workspace: made.workspace,
    containerId: board.id,
    container: board,
    members,
    orderedMembers: members,
    setOpen: vi.fn(),
    setMinimumRole: vi.fn(),
    addMember: vi.fn(),
    setMemberRole: vi.fn(),
    removeMember: vi.fn(),
  } as unknown as Shareable;
  render(ShareDialog, {
    props: { open: true, workspace: made.workspace, shareable, linkOf: (token: string) => token },
  });
  await screen.findByTestId('share-public-section');
  return { invitations, list, onContainer };
}

describe('the share dialog', () => {
  it('has the public link alone in the own space, and asks for nobody', async () => {
    const { invitations, list, onContainer } = await dialogIn(true);
    await waitFor(() => {
      expect(screen.getByTestId('share-public')).toBeEnabled();
    });
    expect(screen.queryByTestId('share-open')).toBeNull();
    expect(screen.queryByTestId('share-members')).toBeNull();
    expect(screen.queryByTestId('share-search')).toBeNull();
    expect(screen.queryByTestId('share-guest-links')).toBeNull();
    expect(screen.queryByRole('heading', { name: /guest/i })).toBeNull();
    expect(invitations).not.toHaveBeenCalled();
    expect(list).not.toHaveBeenCalled();
    expect(onContainer).not.toHaveBeenCalled();
  });

  it('has the workspace, the guests, the members and the teams in a shared workspace', async () => {
    const { invitations, onContainer } = await dialogIn(false);
    expect(screen.getByTestId('share-open')).toBeInTheDocument();
    expect(screen.getByTestId('share-members')).toBeInTheDocument();
    await waitFor(() => {
      expect(invitations).toHaveBeenCalled();
      expect(onContainer).toHaveBeenCalled();
    });
  });
});
