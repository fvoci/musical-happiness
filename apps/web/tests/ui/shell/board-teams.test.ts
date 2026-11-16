/** The teams of a board in its share dialog, against requests that answer as the server would. */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';

import BoardTeams from '$lib/features/shell/BoardTeams.svelte';
import { BackendError, type BoardStore, type Workspace } from '$lib/state/index.ts';
import type { Team, Teams } from '$lib/state/team-types.ts';

afterEach(cleanup);

const DESIGN: Team = { id: 't1', workspaceId: 'w1', name: 'Design', members: [] };

it('puts a refused role back in its select, with the reason', async () => {
  const assign = vi.fn<Teams['assign']>(() => Promise.reject(new BackendError(403)));
  const teams = {
    list: vi.fn<Teams['list']>(() => Promise.resolve([DESIGN])),
    onContainer: vi.fn<Teams['onContainer']>(() =>
      Promise.resolve([{ team: DESIGN, role: 'viewer' }]),
    ),
    assign,
  } as Partial<Teams> as Teams;
  const workspace = { info: { id: 'w1' } } as unknown as Workspace;
  const board = { boardId: 'b1', containerId: 'b1', members: [] } as unknown as BoardStore;
  render(BoardTeams, { workspace, shareable: board, teams });
  const select = await screen.findByRole<HTMLSelectElement>('combobox', { name: /Design/ });
  expect(select.value).toBe('viewer');
  await fireEvent.change(select, { target: { value: 'editor' } });
  expect(assign).toHaveBeenCalledWith('w1', 'b1', 't1', 'editor');
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not change the team.');
  await waitFor(() => {
    expect(select.value).toBe('viewer');
  });
});

it('takes a team off the board by the × named after it', async () => {
  const unassign = vi.fn<Teams['unassign']>(() => Promise.resolve());
  const teams = {
    list: vi.fn<Teams['list']>(() => Promise.resolve([DESIGN])),
    onContainer: vi.fn<Teams['onContainer']>(() =>
      Promise.resolve([{ team: DESIGN, role: 'editor' }]),
    ),
    unassign,
  } as Partial<Teams> as Teams;
  const workspace = { info: { id: 'w1' } } as unknown as Workspace;
  const board = { boardId: 'b1', containerId: 'b1', members: [] } as unknown as BoardStore;
  render(BoardTeams, { workspace, shareable: board, teams });
  const remove = await screen.findByRole('button', { name: 'Remove Design from board' });
  expect(remove).not.toHaveTextContent('Design');
  await fireEvent.click(remove);
  expect(unassign).toHaveBeenCalledWith('w1', 'b1', 't1');
});

it('shows the teams and their roles, unchanged, to a reader who changes nothing', async () => {
  const teams = {
    list: vi.fn<Teams['list']>(() => Promise.resolve([DESIGN])),
    onContainer: vi.fn<Teams['onContainer']>(() =>
      Promise.resolve([{ team: DESIGN, role: 'viewer' }]),
    ),
  } as Partial<Teams> as Teams;
  const workspace = { info: { id: 'w1' } } as unknown as Workspace;
  const board = { boardId: 'b1', containerId: 'b1', members: [] } as unknown as BoardStore;
  render(BoardTeams, { workspace, shareable: board, teams, readOnly: true });
  expect(await screen.findByText('Design')).toBeInTheDocument();
  expect(screen.getByText('Viewer')).toBeInTheDocument();
  expect(screen.queryByRole('combobox')).toBeNull();
  expect(screen.queryByRole('button', { name: /Remove/ })).toBeNull();
});

it('leaves out an empty list of teams for a reader who changes nothing', async () => {
  const onBoard = vi.fn<Teams['onContainer']>(() => Promise.resolve([]));
  const teams = {
    list: vi.fn<Teams['list']>(() => Promise.resolve([DESIGN])),
    onContainer: onBoard,
  } as Partial<Teams> as Teams;
  const workspace = { info: { id: 'w1' } } as unknown as Workspace;
  const board = { boardId: 'b1', containerId: 'b1', members: [] } as unknown as BoardStore;
  render(BoardTeams, { workspace, shareable: board, teams, readOnly: true });
  await waitFor(() => {
    expect(onBoard).toHaveBeenCalled();
  });
  expect(screen.queryByTestId('board-teams')).toBeNull();
});
