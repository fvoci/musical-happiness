/** SSO settings preserve secrets and explain the server's lockout guards. */
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import OrgSso from '$lib/features/organization/OrgSso.svelte';
import { BackendError } from '$lib/state/backend.ts';
import type { OidcProvider, OidcSettings } from '$lib/state/oidc.ts';

afterEach(cleanup);
const provider: OidcProvider = {
  issuer: 'https://login.example.test',
  clientId: 'client',
  hasClientSecret: true,
  enabled: true,
  autoJoin: true,
  ssoRequired: false,
  canRequireSso: false,
  redirectUri: 'https://app.example.test/api/v1/auth/oidc/callback/org',
  createAt: 1,
  updateAt: 1,
};

function mount(value: OidcProvider | undefined = provider) {
  const save = vi.fn(async (settings: OidcSettings): Promise<OidcProvider> => ({
    ...provider,
    ...settings,
  }));
  const remove = vi.fn(() => Promise.resolve());
  const onSignIn = vi.fn(() => Promise.resolve());
  render(OrgSso, { read: async () => value, save, remove, onSignIn });
  return { save, remove, onSignIn };
}

describe('the SSO settings', () => {
  it('keeps a stored secret out of the form and unchanged on save', async () => {
    const { save } = mount();
    await screen.findByText('Client secret is stored.');
    expect(screen.queryByTestId('sso-secret')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByTestId('sso-save'));
    await vi.waitFor(() => {
      expect(save).toHaveBeenCalledOnce();
    });
    expect(save.mock.calls[0]?.[0]).not.toHaveProperty('clientSecret');
    expect(screen.getByTestId('sso-callback')).toHaveValue(provider.redirectUri);
  });

  it('only enables enforcement for the current provider and session', async () => {
    const { onSignIn } = mount();
    await screen.findByTestId('sso-client');
    expect(screen.getByTestId('sso-required-switch')).toBeDisabled();
    await fireEvent.click(screen.getByTestId('sso-sign-in'));
    expect(onSignIn).toHaveBeenCalledOnce();
  });

  it('says that another issuer signs out who signed in through the current one', async () => {
    mount();
    const issuer = await screen.findByTestId('sso-issuer');
    expect(screen.queryByTestId('sso-issuer-change')).not.toBeInTheDocument();
    await fireEvent.input(issuer, { target: { value: 'https://other.example.test' } });
    expect(screen.getByTestId('sso-issuer-change')).toHaveTextContent('signs out everyone');
    await fireEvent.input(issuer, { target: { value: provider.issuer } });
    expect(screen.queryByTestId('sso-issuer-change')).not.toBeInTheDocument();
  });

  it('changing the client requires proving the replacement too', async () => {
    mount({ ...provider, canRequireSso: true });
    const client = await screen.findByTestId('sso-client');
    expect(screen.getByTestId('sso-required-switch')).toBeEnabled();
    await fireEvent.input(client, { target: { value: 'other-client' } });
    expect(screen.getByTestId('sso-required-switch')).toBeDisabled();
  });

  it('does not disable, replace or delete a provider while enforcement is stored', async () => {
    mount({ ...provider, canRequireSso: true, ssoRequired: true });
    await screen.findByTestId('sso-client');
    for (const id of ['sso-issuer', 'sso-client', 'sso-enabled', 'sso-delete']) {
      expect(screen.getByTestId(id)).toBeDisabled();
    }
    expect(screen.getByTestId('sso-required-switch')).toBeEnabled();
  });

  it('keeps an entered secret for retry and clears it after saving', async () => {
    const { save } = mount();
    save.mockRejectedValueOnce(new BackendError(400, { reason: 'oidc_issuer_unverified' }));
    await screen.findByText('Client secret is stored.');
    await fireEvent.click(screen.getByText('Change secret'));
    await fireEvent.input(screen.getByTestId('sso-secret'), { target: { value: 'replacement' } });
    await fireEvent.click(screen.getByTestId('sso-save'));
    await screen.findByText(/The issuer's discovery could not be verified/);
    expect(save.mock.calls[0]?.[0].clientSecret).toBe('replacement');
    expect(screen.getByTestId('sso-secret')).toHaveValue('replacement');
    await fireEvent.click(screen.getByTestId('sso-save'));
    await vi.waitFor(() => {
      expect(save).toHaveBeenCalledTimes(2);
    });
    expect(save.mock.calls[1]?.[0].clientSecret).toBe('replacement');
    await vi.waitFor(() => {
      expect(screen.queryByTestId('sso-secret')).not.toBeInTheDocument();
    });
    expect(document.body.textContent).not.toContain('replacement');
  });
});
