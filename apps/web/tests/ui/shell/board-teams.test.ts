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
    onBoard: vi.fn<Teams['onBoard']>(() => Promise.resolve([{ team: DESIGN, role: 'viewer' }])),
    assign,
  } as Partial<Teams> as Teams;
  const workspace = { info: { id: 'w1' } } as unknown as Workspace;
  const board = { boardId: 'b1', members: [] } as unknown as BoardStore;
  render(BoardTeams, { workspace, board, teams });
  const select = await screen.findByRole<HTMLSelectElement>('combobox', { name: /Design/ });
  expect(select.value).toBe('viewer');
  await fireEvent.change(select, { target: { value: 'editor' } });
  expect(assign).toHaveBeenCalledWith('w1', 'b1', 't1', 'editor');
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not change the team.');
  await waitFor(() => {
    expect(select.value).toBe('viewer');
  });
});
