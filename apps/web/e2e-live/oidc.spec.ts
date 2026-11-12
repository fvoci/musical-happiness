/**
 * Signing in through the organization's provider on the real server: the
 * fake provider of the server's tests runs beside it (`tools/live-server.ts`),
 * the administrator sets it as the organization's, and somebody who has no
 * account yet signs in with the button of the sign-in page and opens a board.
 */
import { expect, test } from '@playwright/test';

import { ADMIN, invitation, makeBoard, signedIn, watchErrors, type Schemas } from './support.ts';

// The fake provider's certificate is its own.
test.use({ locale: 'en-US', ignoreHTTPSErrors: true });

const PORT = Number(process.env.FVOCI_LIVE_PORT ?? 18510);
/** Where `tools/live-server.ts` runs the fake provider, and its client. */
const ISSUER = `https://127.0.0.1:${PORT + 1}`;
const CLIENT_ID = 'fvoci-test-client';
const CLIENT_SECRET = 'fvoci test secret: not one to keep';

test('a newcomer signs in through the provider and opens a board', async ({ page, baseURL }) => {
  const base = baseURL ?? '';
  await invitation(base);
  const admin = await signedIn(base, ADMIN);
  const [workspace] = (await (
    await admin.get('/api/v1/users/me/workspaces')
  ).json()) as Schemas['UserWorkspace'][];
  const orgId = workspace?.orgId ?? '';
  const set = await admin.put(`/api/v1/org/${orgId}/oidc`, {
    data: { issuer: ISSUER, clientId: CLIENT_ID, clientSecret: CLIENT_SECRET },
  });
  expect(set.ok(), await set.text()).toBe(true);
  const provider = (await set.json()) as Schemas['OidcProvider'];
  expect(provider.hasClientSecret).toBe(true);
  expect(JSON.stringify(provider)).not.toContain(CLIENT_SECRET);
  const board = await makeBoard(admin, { title: 'Through the provider', type: 'O' });

  const errors = watchErrors(page);
  await page.goto('/');
  const button = page.getByTestId('oidc-continue');
  await expect(button).toHaveText('Continue with single sign-on');
  await button.click();
  // Signed in: the app, wherever it opens, and not the sign-in page.
  await expect(page.getByTestId('app-shell')).toBeVisible();
  expect(new URL(page.url()).pathname).not.toBe('/login');

  await page.goto(`/board/${board.boardId}/${board.viewId}`);
  const join = page.getByTestId('board-join');
  await expect(join.getByRole('heading')).toHaveText('Through the provider');
  await join.getByRole('button', { name: 'Join this board' }).click();
  await expect(page.getByTestId('board-page')).toBeVisible();
  expect(errors).toEqual([]);
  await admin.dispose();
});

test('a sign-in through the provider that went wrong says why', async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto('/login?oidc_error=oidc-email-taken');
  await expect(page.getByTestId('sign-in-form').getByRole('alert')).toHaveText(
    'An account with your email address exists already. Sign in to it with its password.',
  );
  expect(errors).toEqual([]);
});
