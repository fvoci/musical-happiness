/** The teams of a workspace, against requests that answer as the server would. */
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';

import TeamsPanel from '$lib/features/shell/TeamsPanel.svelte';
import { BackendError, type People, type Workspace } from '$lib/state/index.ts';
import type { Team, Teams } from '$lib/state/team-types.ts';

afterEach(cleanup);

const REFUSALS: [problem: BackendError, told: RegExp][] = [
  [
    new BackendError(400, {
      errors: [{ pointer: '/name', reason: 'too_long', detail: 'too long' }],
    }),
    /up to 64 characters/,
  ],
  [
    new BackendError(400, {
      errors: [{ pointer: '/name', reason: 'invalid_characters', detail: 'invisible' }],
    }),
    /cannot have invisible characters/,
  ],
  [new BackendError(409, { reason: 'team_name_taken' }), /Letter case does not make/],
];

it.each(REFUSALS)(
  'keeps a refused name typed and says why in one line (%#)',
  async (problem, told) => {
    const teams = {
      list: vi.fn<Teams['list']>(() => Promise.resolve([])),
      create: vi.fn<Teams['create']>(() => Promise.reject(problem)),
    } as Partial<Teams> as Teams;
    const people = { members: () => Promise.resolve([]) } as unknown as People;
    const workspace = {
      info: { id: 'w1', role: 'admin', title: 'Studio' },
    } as unknown as Workspace;
    render(TeamsPanel, { workspace, people, teams });
    const input = await screen.findByRole('textbox', { name: 'Team name' });
    await fireEvent.input(input, { target: { value: 'DESIGN' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Create team' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(told);
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(input).toHaveValue('DESIGN');
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(input).toHaveAccessibleDescription(told);
  },
);

it('says why a new name of a team is refused, and keeps the rename box with what was typed', async () => {
  const design: Team = { id: 't1', workspaceId: 'w1', name: 'Design', members: [] };
  const teams = {
    list: vi.fn<Teams['list']>(() => Promise.resolve([design])),
    rename: vi.fn<Teams['rename']>(() =>
      Promise.reject(new BackendError(409, { reason: 'team_name_taken' })),
    ),
  } as Partial<Teams> as Teams;
  const people = { members: () => Promise.resolve([]) } as unknown as People;
  const workspace = { info: { id: 'w1', role: 'admin', title: 'Studio' } } as unknown as Workspace;
  render(TeamsPanel, { workspace, people, teams });
  await fireEvent.click(await screen.findByText('Design'));
  await fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
  const box = screen.getByRole('textbox', { name: 'New name for Design' });
  await fireEvent.input(box, { target: { value: 'RESEARCH' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(/Letter case does not make/);
  expect(box).toHaveValue('RESEARCH');
  expect(box).toHaveAttribute('aria-invalid', 'true');
  expect(box).toHaveAccessibleDescription(/already exists here/);
  expect(screen.getByRole('textbox', { name: 'Team name' })).not.toHaveAttribute('aria-invalid');
});
