/**
 * The connected sign-ins of the settings, the page of a workspace that asks
 * for single sign-on, and the notice of a sign-in that came back refused:
 * what each says, and what its buttons ask for.
 */
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import LinkedSignIns from '$lib/features/account/LinkedSignIns.svelte';
import { m } from '$lib/i18n/paraglide/messages.js';
import SignInNotice from '$lib/features/account/SignInNotice.svelte';
import SsoRequired from '$lib/features/account/SsoRequired.svelte';
import { BackendError, type LinkedIdentity } from '$lib/state/index.ts';

afterEach(() => {
  cleanup();
});

const IDENTITY: LinkedIdentity = {
  issuer: 'https://login.example.com/realm',
  subject: 'there',
  email: 'ada@example.com',
  createAt: 1,
};

function mount(
  identities: LinkedIdentity[],
  refuse?: BackendError,
  organizations = [
    { orgId: 'org-a', name: 'Acme' },
    { orgId: 'org-b', name: 'Beta' },
  ],
) {
  const read = vi.fn(() => Promise.resolve(identities));
  const act = (): Promise<void> => (refuse ? Promise.reject(refuse) : Promise.resolve());
  const onLink = vi.fn((_orgId: string, _password: string) => act());
  const onUnlink = vi.fn((_identity: LinkedIdentity, _password: string) => act());
  render(LinkedSignIns, {
    props: { read, organizations: () => Promise.resolve(organizations), onLink, onUnlink },
  });
  return { read, onLink, onUnlink };
}

describe('the connected sign-ins', () => {
  it('lists what is connected by the host of its provider and the address it gave', async () => {
    mount([IDENTITY]);
    const row = await screen.findByTestId('linked-sign-in');
    expect(row).toHaveTextContent('login.example.com');
    expect(row).toHaveTextContent('ada@example.com');
    expect(within(row).getByRole('button', { name: 'Disconnect' })).toBeInTheDocument();
  });

  it('connects through the organization chosen, with the password', async () => {
    const { onLink } = mount([]);
    expect(await screen.findByText('Nothing is connected yet.')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Connect single sign-on' }));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'org-b' } });
    await fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'the password' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Continue to single sign-on' }));
    expect(onLink).toHaveBeenCalledExactlyOnceWith('org-b', 'the password');
  });

  it('offers nothing to connect when no organization of the reader has a provider', async () => {
    mount([], undefined, []);
    expect(await screen.findByText(m.account_linked_none_offered())).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Connect single sign-on' })).toBeNull();
  });

  it('disconnects with the password, reads again, and says so', async () => {
    const { read, onUnlink } = mount([IDENTITY]);
    const row = await screen.findByTestId('linked-sign-in');
    await fireEvent.click(within(row).getByRole('button', { name: 'Disconnect' }));
    await fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'the password' },
    });
    read.mockResolvedValue([]);
    await fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }));
    expect(onUnlink).toHaveBeenCalledExactlyOnceWith(IDENTITY, 'the password');
    expect(await screen.findByText('Disconnected.')).toBeInTheDocument();
    expect(await screen.findByText('Nothing is connected yet.')).toBeInTheDocument();
  });

  it.each([
    ['invalid_credentials', 400, 'The current password is wrong.'],
    ['last_sign_in', 409, 'This is the only way to sign in to this account, so it stays.'],
    ['password_not_set', 409, 'This account has no password to confirm this with.'],
  ] as const)('says why a disconnection was refused: %s', async (reason, status, words) => {
    mount([IDENTITY], new BackendError(status, { reason }));
    const row = await screen.findByTestId('linked-sign-in');
    await fireEvent.click(within(row).getByRole('button', { name: 'Disconnect' }));
    await fireEvent.input(screen.getByLabelText('Current password'), { target: { value: 'x' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(words);
    expect(screen.getByTestId('linked-sign-in')).toBeInTheDocument();
  });
});

describe('a workspace that asks for single sign-on', () => {
  it('says so by its name, and goes to the provider', async () => {
    const onContinue = vi.fn(() => Promise.resolve());
    const onOpenLinked = vi.fn();
    render(SsoRequired, { props: { name: 'Design', onContinue, onOpenLinked } });
    expect(screen.getByRole('heading')).toHaveTextContent('Design signs in with single sign-on');
    await fireEvent.click(screen.getByRole('button', { name: 'Continue with single sign-on' }));
    expect(onContinue).toHaveBeenCalledOnce();
    await fireEvent.click(screen.getByRole('button', { name: 'Connected sign-ins' }));
    expect(onOpenLinked).toHaveBeenCalledOnce();
  });

  it('says that the start did not work, and can be tried again', async () => {
    const onContinue = vi.fn(() => Promise.reject(new BackendError(409)));
    render(SsoRequired, { props: { name: '', onContinue, onOpenLinked: vi.fn() } });
    expect(screen.getByRole('heading')).toHaveTextContent(m.account_sso_required_title());
    const button = screen.getByRole('button', { name: 'Continue with single sign-on' });
    await fireEvent.click(button);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Signing in through the provider did not work.',
    );
    expect(button).toBeEnabled();
  });
});

describe('a sign-in that came back refused to somebody who is signed in', () => {
  it('says why, with the way to the connected sign-ins and the way on', async () => {
    const onGoOn = vi.fn();
    const onOpenLinked = vi.fn();
    render(SignInNotice, { props: { problem: 'oidc-identity-taken', onGoOn, onOpenLinked } });
    expect(screen.getByRole('alert')).toHaveTextContent(
      'That sign-in belongs to another account already. Nothing was connected.',
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Connected sign-ins' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Go on' }));
    expect(onOpenLinked).toHaveBeenCalledOnce();
    expect(onGoOn).toHaveBeenCalledOnce();
  });
});
