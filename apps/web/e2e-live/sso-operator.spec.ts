/**
 * Single sign-on from the organization's screen to the operator's release,
 * on the real server with the fake provider beside it
 * (`tools/live-server.ts`): the administrator sets the provider up on the
 * screen, signs in through it and requires it; a member's password session
 * is told so; the person who runs the server releases the organization on
 * the administration socket, and the organization's records name the server
 * operator; the administrator disconnects the sign-in their session was made
 * through, is told beforehand that it signs them out, and is signed out.
 *
 * It runs with the test that closes the organization to passwords, after the
 * others (`playwright.live.config.ts`): the provider and the rule are taken
 * away again at the end, whatever came of the test.
 */
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { expect, request, test, type Page } from '@playwright/test';

import {
  ADMIN,
  invitation,
  operator,
  person,
  register,
  signedIn,
  signInOnPage,
  uniqueName,
  type Schemas,
} from './support.ts';

// The fake provider's certificate is its own.
test.use({ locale: 'en-US', ignoreHTTPSErrors: true });

const PORT = Number(process.env.FVOCI_LIVE_PORT ?? 18510);
const ISSUER = `https://127.0.0.1:${PORT + 1}`;
const CLIENT_ID = 'fvoci-test-client';
const SECRET = 'fvoci test secret: not one to keep';

/** Who the provider signs in from now on. */
async function providerSignsIn(who: {
  subject: string;
  email: string;
  preferredUsername?: string;
  name?: string;
}): Promise<void> {
  const provider = await request.newContext({ ignoreHTTPSErrors: true });
  expect((await provider.post(`${ISSUER}/person`, { data: who })).status()).toBe(204);
  await provider.dispose();
}

/** The page at both widths, under `target/screens/sso/`, with nothing wider than the window. */
async function pictures(page: Page, name: string): Promise<void> {
  const directory = path.resolve('../../target/screens/sso');
  await mkdir(directory, { recursive: true });
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.screenshot({ path: path.join(directory, `${name}-${width}.png`), fullPage: true });
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      width,
    );
  }
  await page.setViewportSize({ width: 1280, height: 900 });
}

test('an administrator requires single sign-on on the screen, the operator releases it, and disconnecting signs out', async ({
  page,
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  await invitation(base);
  const member = person('operator-member');
  await register(base, member);
  const api = await signedIn(base, ADMIN);
  const [workspace] = (await (
    await api.get('/api/v1/users/me/workspaces')
  ).json()) as Schemas['UserWorkspace'][];
  const orgId = workspace?.orgId ?? '';
  const endpoint = `/api/v1/org/${orgId}/oidc`;
  const memberContext = await browser.newContext({ locale: 'en-US' });

  try {
    // The member works with their password before the organization takes none.
    const memberPage = await memberContext.newPage();
    await memberPage.goto('/');
    await signInOnPage(memberPage, member);
    await expect(memberPage.getByTestId('app-shell')).toBeVisible();

    // The administrator puts the provider in on the organization's screen.
    await page.goto('/');
    await signInOnPage(page, ADMIN);
    await page.goto(`/org/${orgId}`);
    const section = page.getByTestId('org-sso');
    await section.getByTestId('sso-issuer').fill(ISSUER);
    await section.getByTestId('sso-client').fill(CLIENT_ID);
    await section.getByTestId('sso-secret').fill(SECRET);
    await section.getByTestId('sso-save').click();
    await expect(section.getByRole('status')).toContainText('Saved.');
    await expect(section.getByTestId('sso-required-switch')).toBeDisabled();

    // Connects their account to it, and signs in through it from the section.
    await providerSignsIn({ subject: uniqueName('operator-admin'), email: ADMIN.email });
    const started = await page.request.post(`/api/v1/auth/oidc/start?org=${orgId}`, {
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
      data: { linkPassword: ADMIN.password },
    });
    expect(started.ok(), await started.text()).toBe(true);
    const { authorizationUrl } = (await started.json()) as Schemas['OidcStart'];
    expect((await page.request.get(authorizationUrl)).url()).toContain('oidc_linked=1');
    await section.getByTestId('sso-sign-in').click();
    await expect(section.getByTestId('sso-required-switch')).toBeEnabled();

    // Requires it: the provider cannot be removed while it is required.
    await section.getByTestId('sso-required-switch').click();
    await section.getByTestId('sso-save').click();
    await expect(section.getByTestId('sso-delete')).toBeDisabled();

    // The member's password session is told so in place of what they had open.
    await memberPage.reload();
    await expect(memberPage.getByTestId('sso-required')).toBeVisible();
    await expect(memberPage.getByTestId('sign-in-form')).toHaveCount(0);

    // The person who runs the server releases it: the records say the server operator did.
    expect(await operator(base, `orgs/${orgId}/sso/release`)).toBe(204);
    await page.reload();
    await expect(section.getByTestId('sso-required-switch')).not.toBeChecked();
    const records = page.getByTestId('org-records');
    await expect(records).toContainText(
      "Server operator released the organization's single sign-on requirement",
    );
    await expect(records).toContainText('required single sign-on for the organization');
    await records.getByText('Server operator released').scrollIntoViewIfNeeded();
    await pictures(page, 'records');
    await memberPage.reload();
    await expect(memberPage.getByTestId('sso-required')).toHaveCount(0);
    await expect(memberPage.getByTestId('app-shell')).toBeVisible();

    // Disconnecting the sign-in this session was made through says first that it
    // signs out, then does.
    await page.goto('/');
    await page.getByTestId('open-settings').click();
    const linked = page.getByTestId('settings-dialog').getByTestId('linked-sign-ins');
    await linked.getByTestId('linked-sign-in').getByRole('button', { name: 'Disconnect' }).click();
    await expect(linked.getByTestId('linked-ends-session')).toContainText(
      'disconnecting it signs you out',
    );
    await pictures(page, 'disconnect');
    await linked.getByLabel('Current password').fill(ADMIN.password);
    await linked.getByRole('button', { name: 'Disconnect it' }).click();
    await expect(page.getByTestId('sign-in-form')).toBeVisible();
    // The password session beside it goes on, and the identity is gone.
    const left = (await (
      await api.get('/api/v1/users/me/identities')
    ).json()) as Schemas['LinkedIdentity'][];
    expect(left).toEqual([]);
  } finally {
    await memberContext.close();
    // The other tests' people sign in with passwords: each step is taken
    // whatever came of the one before, and one that failed is told.
    expect.soft(await operator(base, `orgs/${orgId}/sso/release`), 'released').toBe(204);
    if ((await api.get(endpoint)).ok()) {
      expect.soft((await api.delete(endpoint)).status(), 'the provider is removed').toBe(204);
    }
    await api.dispose();
    await providerSignsIn({
      subject: 'oidc-person',
      email: 'oidc-person@example.test',
      preferredUsername: 'oidc-person',
      name: 'OIDC Person',
    });
  }
});
