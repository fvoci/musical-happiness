/**
 * Personal API tokens in the settings: the token a form makes at first, what
 * it sends, the command a token made is shown once as, what a refusal is told
 * as, and the list that ends one.
 */
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import ApiTokensSection from '$lib/features/account/ApiTokensSection.svelte';
import {
  defaultTokenName,
  lastUsedOf,
  mcpCommand,
  tokenProblemOf,
  tokenWorkspaces,
} from '$lib/features/account/api-tokens.ts';
import { refusedMember } from '$lib/state/backend.ts';
import {
  BackendError,
  type ApiToken,
  type IssuedApiToken,
  type NewApiToken,
  type WorkspaceInfo,
} from '$lib/state/index.ts';

afterEach(cleanup);

const DAY = 24 * 60 * 60 * 1000;

const made = (over: Partial<ApiToken> = {}): ApiToken => ({
  id: 'tok-1',
  userId: 'me',
  name: 'Agent · 11/27',
  tokenPrefix: 'fvoci_pat_abcdef',
  scope: 'write',
  workspaces: [{ workspaceId: 'ws-1', orgId: 'org-1', name: 'Studio' }],
  allowPersonalSpace: false,
  createdAt: Date.UTC(2026, 10, 27),
  expiresAt: Date.UTC(2026, 10, 27) + 90 * DAY,
  ...over,
});

function section(
  options: {
    tokens?: ApiToken[];
    issue?: (token: NewApiToken) => Promise<IssuedApiToken>;
    revoke?: (tokenId: string) => Promise<void>;
  } = {},
) {
  const issue = vi.fn(
    options.issue ??
      ((token: NewApiToken) =>
        Promise.resolve({
          token: 'fvoci_pat_secret',
          apiToken: made({ id: 'tok-2', name: token.name, scope: token.scope }),
        })),
  );
  const revoke = vi.fn(options.revoke ?? (() => Promise.resolve()));
  render(ApiTokensSection, {
    props: {
      read: () => Promise.resolve(options.tokens ?? []),
      issue,
      revoke,
      workspaces: [
        { id: 'ws-1', name: 'Studio' },
        { id: 'ws-2', name: 'Lab' },
      ],
      currentWorkspaceId: 'ws-1',
      hasOwnSpace: true,
      hasPassword: true,
      origin: 'https://fvoci.example',
    },
  });
  return { issue, revoke };
}

describe('the logic of tokens', () => {
  it('names a token by the day it is made', () => {
    expect(defaultTokenName(new Date(2026, 10, 27))).toBe('Agent · 11/27');
  });

  it('puts the token in a header of the command, never in an address', () => {
    expect(mcpCommand('https://fvoci.example', 'fvoci_pat_x')).toBe(
      'claude mcp add --transport http fvoci https://fvoci.example/api/v1/mcp --header "Authorization: Bearer fvoci_pat_x"',
    );
  });

  it('gives a token no own space and no workspace of a guest', () => {
    const one = (over: Partial<WorkspaceInfo>): WorkspaceInfo => ({
      id: 'ws',
      title: 'W',
      ...over,
    });
    expect(
      tokenWorkspaces([
        one({ id: 'own', kind: 'personal' }),
        one({ id: 'guest', role: 'guest' }),
        one({ id: 'ws-1', title: 'Studio', role: 'member' }),
      ]),
    ).toEqual([{ id: 'ws-1', name: 'Studio' }]);
  });

  it('tells a refusal by its reason and the member it points at', () => {
    const refusal = (status: 400 | 403 | 429, extra: object) =>
      new BackendError(status, { detail: 'no', ...extra });
    expect(tokenProblemOf(refusal(403, { reason: 'invalid_credentials' }))).toEqual({
      text: 'The current password is wrong.',
      field: 'password',
    });
    expect(tokenProblemOf(refusal(403, { reason: 'api_tokens_disallowed' })).text).toBe(
      'The organization of a workspace you chose does not allow API tokens.',
    );
    expect(
      tokenProblemOf(refusal(400, { errors: [refusedMember('name', 'no', 'too_long')] })),
    ).toEqual({ text: 'A name has 1 to 80 characters.', field: 'name' });
    expect(tokenProblemOf(refusal(429, {})).field).toBeUndefined();
  });
});

describe('the settings', () => {
  it('make a read-only token of the workspace shown, for 90 days, unless asked otherwise', async () => {
    const { issue } = section();
    await fireEvent.click(await screen.findByRole('button', { name: 'New token' }));
    expect(screen.getByLabelText('Read only')).toBeChecked();
    expect(screen.getByLabelText('Name')).toHaveValue(defaultTokenName(new Date()));
    expect(screen.getByTestId('api-token-form')).toHaveTextContent('This workspace: Studio');
    await fireEvent.click(screen.getByLabelText('Read and write content'));
    await fireEvent.click(screen.getByRole('button', { name: 'Make' }));
    expect(issue).toHaveBeenCalledWith({
      name: defaultTokenName(new Date()),
      scope: 'write',
      workspaceIds: ['ws-1'],
      expiresInDays: 90,
      allowPersonalSpace: false,
      password: undefined,
    });

    // Shown once, as the command, with the line on the shell's history.
    const issued = await screen.findByTestId('api-token-issued');
    expect(issued).toHaveFocus();
    expect(within(issued).getByTestId('api-token-command')).toHaveTextContent(
      'claude mcp add --transport http fvoci https://fvoci.example/api/v1/mcp --header "Authorization: Bearer fvoci_pat_secret"',
    );
    expect(issued).toHaveTextContent('The command stays in your shell history.');
    await fireEvent.click(within(issued).getByRole('button', { name: 'Done' }));
    expect(screen.queryByTestId('api-token-issued')).toBeNull();
    expect(screen.queryByText(/fvoci_pat_secret/)).toBeNull();
  });

  it('give more workspaces and the own space, with the password, under "More"', async () => {
    const { issue } = section();
    await fireEvent.click(await screen.findByRole('button', { name: 'New token' }));
    await fireEvent.click(screen.getByRole('button', { name: 'More' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Lab' }));
    await fireEvent.click(
      screen.getByRole('switch', { name: 'My own space too (not its diaries)' }),
    );
    await fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'pass phrase' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Make' }));
    expect(issue).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceIds: ['ws-1', 'ws-2'],
        allowPersonalSpace: true,
        password: 'pass phrase',
      }),
    );
  });

  it('tell a wrong password at its field and make nothing', async () => {
    section({
      issue: () =>
        Promise.reject(new BackendError(403, { detail: 'no', reason: 'invalid_credentials' })),
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'New token' }));
    await fireEvent.click(screen.getByRole('button', { name: 'More' }));
    await fireEvent.click(
      screen.getByRole('switch', { name: 'My own space too (not its diaries)' }),
    );
    await fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'wrong' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Make' }));
    expect(await screen.findByText('The current password is wrong.')).toBeInTheDocument();
    expect(screen.getByLabelText('Current password')).toHaveFocus();
    expect(screen.queryByTestId('api-token-issued')).toBeNull();
  });

  it('list what each token may do, where and until when, and end one', async () => {
    const { revoke } = section({ tokens: [made()] });
    const row = await screen.findByTestId('api-token');
    expect(row).toHaveTextContent('Read and write content · Studio');
    expect(row).toHaveTextContent('fvoci_pat_abcdef…');
    expect(row).toHaveTextContent('Not used yet');
    const dates = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeZone: 'UTC' });
    expect(
      lastUsedOf(
        made({ lastUsedAt: Date.UTC(2026, 10, 28), lastUsedAddress: '203.0.113.7' }),
        dates,
      ),
    ).toBe('Last used Nov 28, 2026 from 203.0.113.7');
    await fireEvent.click(within(row).getByRole('button', { name: 'End: Agent · 11/27' }));
    expect(revoke).toHaveBeenCalledWith('tok-1');
    expect(await screen.findByText('The token is ended.')).toBeInTheDocument();
    expect(screen.queryByTestId('api-token')).toBeNull();
  });
});
