/**
 * The sign-in page and the organization's provider: the button that continues
 * through it, named after the organization, for the organization the address
 * names; and what a sign-in that came back refused is told as.
 */
import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import SignInPage from '$lib/features/account/SignInPage.svelte';
import type { LoginOrganization } from '$lib/state/index.ts';

afterEach(() => {
  cleanup();
  history.replaceState(null, '', '/');
});

function mount(address: string, found: LoginOrganization | undefined) {
  history.replaceState(null, '', address);
  const organizationOf = vi.fn(async (_orgId: string | undefined) => found);
  render(SignInPage, {
    props: {
      onSignIn: async () => {},
      onRegister: async () => {},
      organizationOf,
      oidcStartUrl: (orgId: string) => `/api/v1/auth/oidc/start?org=${orgId}`,
    },
  });
  return { organizationOf };
}

describe('the provider of the organization on the sign-in page', () => {
  it('is a link named after the organization, to the start of a sign-in for it', async () => {
    const { organizationOf } = mount('/login?org=org-1', {
      id: 'org-1',
      name: 'Acme',
      oidc: true,
    });
    const button = await screen.findByRole('link', { name: 'Continue with Acme' });
    expect(button).toHaveAttribute('href', '/api/v1/auth/oidc/start?org=org-1');
    expect(organizationOf).toHaveBeenCalledWith('org-1');
  });

  it('asks for the server’s only organization when the address names none, or an empty one', async () => {
    const { organizationOf } = mount('/login?org=', { id: 'only', name: '', oidc: true });
    expect(
      await screen.findByRole('link', { name: 'Continue with single sign-on' }),
    ).toBeInTheDocument();
    expect(organizationOf).toHaveBeenCalledWith(undefined);
  });

  it('is not there when the organization has no provider', async () => {
    const { organizationOf } = mount('/login?org=org-1', undefined);
    await vi.waitFor(() => expect(organizationOf).toHaveBeenCalled());
    expect(screen.queryByTestId('oidc-continue')).toBeNull();
  });

  it('tells a refusal by its name, and one it does not know in general words', () => {
    mount('/login?oidc_error=oidc-join-closed', undefined);
    expect(screen.getByRole('alert')).toHaveTextContent(
      'This organization does not let new people in through its provider. Ask its administrator for an invitation.',
    );
    cleanup();
    mount('/login?oidc_error=<b>made-up</b>', undefined);
    expect(screen.getByRole('alert')).toHaveTextContent(
      "Signing in through the provider did not work. Try again, or ask the organization's administrator.",
    );
  });
});
