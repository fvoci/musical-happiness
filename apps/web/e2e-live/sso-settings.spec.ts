/** Provider setup through the organization's screen, against the real discovery endpoint. */
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { ADMIN, invitation, signedIn, signInOnPage, uniqueName, type Schemas } from './support.ts';

test.use({ locale: 'en-US', ignoreHTTPSErrors: true });
const port = Number(process.env.FVOCI_LIVE_PORT ?? 18510);
const issuer = `https://127.0.0.1:${port + 1}`;
const clientId = 'fvoci-test-client';
const secret = 'fvoci test secret: not one to keep';

test('an administrator configures SSO without exposing its secret or locking out password sessions', async ({
  page,
  baseURL,
}) => {
  await invitation(baseURL ?? '');
  const api = await signedIn(baseURL ?? '', ADMIN);
  const [workspace] = (await (
    await api.get('/api/v1/users/me/workspaces')
  ).json()) as Schemas['UserWorkspace'][];
  const orgId = workspace?.orgId ?? '';
  const endpoint = `/api/v1/org/${orgId}/oidc`;
  try {
    await page.goto('/');
    await signInOnPage(page, ADMIN);
    await page.goto(`/org/${orgId}`);
    const section = page.getByTestId('org-sso');
    await expect(section).toBeVisible();
    await section.getByTestId('sso-issuer').fill(`${issuer}/not-an-issuer`);
    await section.getByTestId('sso-client').fill(clientId);
    await section.getByTestId('sso-secret').fill(secret);
    await section.getByTestId('sso-save').click();
    await expect(section.getByRole('alert')).toContainText('discovery could not be verified');
    await expect(section.getByTestId('sso-secret')).toHaveValue(secret);
    await section.getByTestId('sso-issuer').fill(issuer);
    await section.getByTestId('sso-secret').fill(secret);
    await expect(section.getByTestId('sso-auto-join')).toHaveAttribute('aria-checked', 'true');
    await section.getByTestId('sso-save').click();
    await expect(section.getByRole('status')).toContainText('Saved.');
    await expect(section.getByTestId('sso-secret')).toHaveCount(0);
    await expect(section.getByTestId('sso-callback')).toHaveValue(
      `${baseURL}/api/v1/auth/oidc/callback/${orgId}`,
    );
    await expect(section.getByTestId('sso-required-switch')).toBeDisabled();
    await expect(section.getByTestId('sso-sign-in')).toBeEnabled();
    const read = await api.get(endpoint);
    expect(await read.text()).not.toContain(secret);
    const unproven = await api.put(endpoint, { data: { issuer, clientId, ssoRequired: true } });
    expect(unproven.status()).toBe(409);
    await section.getByTestId('sso-auto-join').click();
    await section.getByTestId('sso-save').click();
    await expect(section.getByTestId('sso-save')).toBeEnabled();
    expect(((await (await api.get(endpoint)).json()) as Schemas['OidcProvider']).autoJoin).toBe(
      false,
    );
    await pictures(page);
    await page.request.post(`${issuer}/person`, {
      data: { subject: uniqueName('settings-admin'), email: ADMIN.email },
    });
    const started = await page.request.post(`/api/v1/auth/oidc/start?org=${orgId}`, {
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
      data: { linkPassword: ADMIN.password },
    });
    expect(started.ok()).toBe(true);
    const { authorizationUrl } = (await started.json()) as Schemas['OidcStart'];
    expect((await page.request.get(authorizationUrl)).url()).toContain('oidc_linked=1');
    await section.getByTestId('sso-sign-in').click();
    await expect(section.getByTestId('sso-required-switch')).toBeEnabled();
    await section.getByTestId('sso-required-switch').click();
    await section.getByTestId('sso-save').click();
    await expect(section.getByTestId('sso-save')).toBeEnabled();
    await expect(section.getByTestId('sso-delete')).toBeDisabled();
    await expect(section.getByTestId('sso-issuer')).toBeDisabled();
    expect(
      (
        await page.request.delete(endpoint, {
          headers: { 'X-Requested-With': 'XMLHttpRequest' },
        })
      ).status(),
    ).toBe(409);
    await section.getByTestId('sso-required-switch').click();
    await section.getByTestId('sso-save').click();
    await expect(section.getByTestId('sso-delete')).toBeEnabled();
    await section.getByTestId('sso-delete').click();
    await page
      .getByRole('alertdialog')
      .getByRole('button', { name: 'Delete provider', exact: true })
      .click();
    await expect(section.getByRole('status')).toHaveText('The provider was removed.');
  } finally {
    if ((await page.request.get(endpoint)).ok()) {
      await page.request.put(endpoint, {
        headers: { 'X-Requested-With': 'XMLHttpRequest' },
        data: { issuer, clientId, ssoRequired: false },
      });
    }
    await page.request.post(`${issuer}/person`, {
      data: {
        subject: 'oidc-person',
        email: 'oidc-person@example.test',
        preferredUsername: 'oidc-person',
        name: 'OIDC Person',
      },
    });
    expect((await api.delete(endpoint)).status()).toBe(204);
    await api.dispose();
  }
});

async function pictures(page: Page): Promise<void> {
  const directory = path.resolve('../../target/screens/sso');
  await mkdir(directory, { recursive: true });
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.screenshot({ path: path.join(directory, `settings-${width}.png`), fullPage: true });
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      width,
    );
  }
}
