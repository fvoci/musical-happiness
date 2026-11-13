import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import TeamRow from '$lib/features/shell/TeamRow.svelte';

afterEach(cleanup);

it('disables a temporary administrator adding themselves but allows another person', async () => {
  const onAdd = vi.fn(() => Promise.resolve());
  render(TeamRow, {
    team: { id: 'team', workspaceId: 'workspace', name: 'Design', members: [] },
    members: [
      { userId: 'me', username: 'ada', displayName: 'Ada', role: 'admin' },
      { userId: 'other', username: 'grace', displayName: 'Grace', role: 'member' },
    ],
    managing: true,
    working: false,
    restrictedSelf: 'me',
    onAdd,
    onRename: () => Promise.resolve(),
    onRemove: () => Promise.resolve(),
    onDelete: vi.fn(),
  });
  await fireEvent.click(screen.getByText('Design'));
  expect(screen.getByRole('option', { name: /Ada/ })).toBeDisabled();
  expect(screen.getByRole('option', { name: /Grace/ })).toBeEnabled();
  expect(screen.getByText(/You cannot add yourself to a team/)).toBeInTheDocument();
  await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'other' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Add a person' }));
  expect(onAdd).toHaveBeenCalledWith('other');
});
