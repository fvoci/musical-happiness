/**
 * The sign-in page and the organization's provider: the button that starts a
 * sign-in through it (a request of the page, not an address another page could
 * send a browser to), named after the organization, for the organization the
 * address names; and what a start or a sign-in that came back refused is told as.
 */
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import SignInPage from '$lib/features/account/SignInPage.svelte';
import { BackendError, type LoginOrganization } from '$lib/state/index.ts';

afterEach(() => {
  cleanup();
  history.replaceState(null, '', '/');
});

function mount(
  address: string,
  found: LoginOrganization | undefined,
  start: (orgId: string) => Promise<void> = () => Promise.resolve(),
) {
  history.replaceState(null, '', address);
  const organizationOf = vi.fn(async (_orgId: string | undefined) => ({
    ...(found ? { organization: found } : {}),
    registrationOpen: true,
  }));
  const onOidcStart = vi.fn(start);
  render(SignInPage, {
    props: {
      onSignIn: vi.fn(() => Promise.resolve()),
      onRegister: vi.fn(() => Promise.resolve('made' as const)),
      optionsOf: organizationOf,
      onOidcStart,
    },
  });
  return { organizationOf, onOidcStart };
}

describe('the provider of the organization on the sign-in page', () => {
  it('is a button named after the organization that starts a sign-in for it', async () => {
    const { organizationOf, onOidcStart } = mount('/login?org=org-1', {
      id: 'org-1',
      name: 'Acme',
      oidc: true,
    });
    const button = await screen.findByRole('button', { name: 'Continue with Acme' });
    expect(button).not.toHaveAttribute('href');
    expect(organizationOf).toHaveBeenCalledWith('org-1');
    await fireEvent.click(button);
    expect(onOidcStart).toHaveBeenCalledExactlyOnceWith('org-1');
  });

  it('says why a start was refused', async () => {
    mount('/login?org=org-1', { id: 'org-1', name: 'Acme', oidc: true }, () =>
      Promise.reject(new BackendError(409, { detail: 'no', reason: 'oidc_unavailable' })),
    );
    await fireEvent.click(await screen.findByRole('button', { name: 'Continue with Acme' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Signing in through single sign-on did not work. Try again, or ask your organization's administrator.",
    );
  });

  it('asks for the server’s only organization when the address names none, or an empty one', async () => {
    const { organizationOf } = mount('/login?org=', { id: 'only', name: '', oidc: true });
    expect(
      await screen.findByRole('button', { name: 'Continue with single sign-on' }),
    ).toBeInTheDocument();
    expect(organizationOf).toHaveBeenCalledWith(undefined);
  });

  it('is not there when the organization has no provider', async () => {
    const { organizationOf } = mount('/login?org=org-1', {
      id: 'org-1',
      name: 'Acme',
      oidc: false,
    });
    await vi.waitFor(() => {
      expect(organizationOf).toHaveBeenCalled();
    });
    // The answer read and the page drawn after it: then there is no button.
    await organizationOf.mock.results[0]?.value;
    await tick();
    await tick();
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeInTheDocument();
    expect(screen.queryByTestId('oidc-continue')).toBeNull();
  });

  it('tells a refusal by its name, and one it does not know in general words', () => {
    mount('/login?oidc_error=oidc-join-closed', undefined);
    expect(screen.getByRole('alert')).toHaveTextContent(
      'This organization does not let new people in through its single sign-on. Ask its administrator for an invitation.',
    );
    cleanup();
    mount('/login?oidc_error=<b>made-up</b>', undefined);
    expect(screen.getByRole('alert')).toHaveTextContent(
      "Signing in through single sign-on did not work. Try again, or ask your organization's administrator.",
    );
  });
});
