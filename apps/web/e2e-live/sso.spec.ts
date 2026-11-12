/**
 * An organization that takes no password, on the real server with the fake
 * provider of its tests beside it (`tools/live-server.ts`): a member connects
 * its single sign-on to their account in the settings; the organization then
 * begins to take no password; the member, still signed in with theirs, is
 * told so in place of the board, goes through the single sign-on, and is
 * back on the board they asked for, in the same account.
 *
 * The server of these tests has one organization, which every test's people
 * are in: the provider and the rule are taken away again at the end, whatever
 * came of the test.
 */
import { expect, request, test, type APIRequestContext } from '@playwright/test';

import {
  ADMIN,
  boardPath,
  invitation,
  makeBoard,
  person,
  register,
  signInOnPage,
  uniqueName,
  watchErrors,
  type Schemas,
} from './support.ts';

// The fake provider's certificate is its own.
test.use({ locale: 'en-US', ignoreHTTPSErrors: true });

const PORT = Number(process.env.FVOCI_LIVE_PORT ?? 18510);
/** Where `tools/live-server.ts` runs the fake provider, and its client. */
const ISSUER = `https://127.0.0.1:${PORT + 1}`;
const CLIENT_ID = 'fvoci-test-client';
const CLIENT_SECRET = 'fvoci test secret: not one to keep';
const PROVIDER = { issuer: ISSUER, clientId: CLIENT_ID };

/** A client of the API that takes the fake provider's certificate, signed in as nobody yet. */
async function trustingClient(baseURL: string): Promise<APIRequestContext> {
  return request.newContext({
    baseURL,
    ignoreHTTPSErrors: true,
    extraHTTPHeaders: { 'X-Requested-With': 'XMLHttpRequest' },
  });
}

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

/** Goes through the provider as a browser does, in `api`: started here, and back to the server. */
async function throughProvider(
  api: APIRequestContext,
  orgId: string,
  linkPassword?: string,
): Promise<string> {
  const started = await api.post(`/api/v1/auth/oidc/start?org=${orgId}`, {
    data: linkPassword === undefined ? undefined : { linkPassword },
  });
  expect(started.ok(), await started.text()).toBe(true);
  const { authorizationUrl } = (await started.json()) as Schemas['OidcStart'];
  const back = await api.get(authorizationUrl);
  expect(back.ok(), back.url()).toBe(true);
  return back.url();
}

test('what takes no password is opened through its single sign-on, back on the board asked for', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  await invitation(base);
  const member = person('sso-member');
  await register(base, member);
  const admin = await trustingClient(base);
  const login = await admin.post('/api/v1/login', {
    data: { username: ADMIN.username, password: ADMIN.password },
  });
  expect(login.ok(), await login.text()).toBe(true);
  const [workspace] = (await (
    await admin.get('/api/v1/users/me/workspaces')
  ).json()) as Schemas['UserWorkspace'][];
  const orgId = workspace?.orgId ?? '';
  const oidc = `/api/v1/org/${orgId}/oidc`;
  const set = await admin.put(oidc, { data: { ...PROVIDER, clientSecret: CLIENT_SECRET } });
  expect(set.ok(), await set.text()).toBe(true);
  const board = await makeBoard(admin, {
    title: 'Behind single sign-on',
    members: [{ who: member, role: 'editor' }],
    cards: [{ title: 'Kept behind the wall' }],
  });
  const address = boardPath(board, board.viewId);
  const memberThere = { subject: uniqueName('member-there'), email: member.email };
  const adminThere = { subject: uniqueName('admin-there'), email: ADMIN.email };
  /** The administrator's session made through the provider, once there is one. */
  let adminThrough: APIRequestContext | undefined;

  try {
    const errors = watchErrors(page);
    await page.goto(address);
    await signInOnPage(page, member);
    await expect(page.getByTestId('board-page')).toBeVisible();

    // The member connects the organization's single sign-on to their account.
    await providerSignsIn(memberThere);
    await page.getByTestId('open-settings').click();
    const linked = page.getByTestId('settings-dialog').getByTestId('linked-sign-ins');
    await expect(linked.getByText('Nothing is connected yet.')).toBeVisible();
    await linked.getByTestId('linked-connect').click();
    await linked.getByLabel('Current password').fill(member.password);
    await linked.getByRole('button', { name: 'Continue to single sign-on' }).click();
    // To the provider and back: the settings say it is connected, on the board they left.
    await expect(linked.getByRole('status')).toHaveText(
      'Connected. Single sign-on now signs in to this account.',
    );
    await expect(linked.getByTestId('linked-sign-in')).toContainText(`127.0.0.1:${PORT + 1}`);
    expect(new URL(page.url()).pathname).toBe(address);
    expect(new URL(page.url()).search).toBe('');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('board-page')).toBeVisible();

    // The administrator does the same, signs in through it, and switches passwords off.
    await providerSignsIn(adminThere);
    expect(await throughProvider(admin, orgId, ADMIN.password)).toContain('oidc_linked=1');
    adminThrough = await trustingClient(base);
    await throughProvider(adminThrough, orgId);
    const unproven = await admin.put(oidc, { data: { ...PROVIDER, ssoRequired: true } });
    expect(unproven.status()).toBe(409);
    const required = await adminThrough.put(oidc, { data: { ...PROVIDER, ssoRequired: true } });
    expect(required.ok(), await required.text()).toBe(true);

    // The member's password session: told so in place of the board, and not signed out.
    await providerSignsIn(memberThere);
    await page.reload();
    const closed = page.getByTestId('sso-required');
    await expect(closed.getByRole('heading')).toContainText('signs in with single sign-on');
    await expect(page.getByTestId('board-page')).toHaveCount(0);
    await expect(page.getByTestId('sign-in-form')).toHaveCount(0);
    await expect(page.getByTestId('app-shell')).toBeVisible();

    // Through the single sign-on: the same account, on the board asked for.
    await closed.getByTestId('sso-continue').click();
    await expect(page.getByTestId('board-page')).toBeVisible();
    expect(new URL(page.url()).pathname).toBe(address);
    await expect(page.getByText('Kept behind the wall')).toBeVisible();
    await page.getByTestId('open-settings').click();
    await expect(page.getByTestId('signed-in-as')).toContainText(member.username);
    expect(errors).toEqual([]);
  } finally {
    // The other tests' people sign in with passwords, and to pages without a
    // provider: each step is taken whatever came of the one before, and one
    // that failed is told, as the files after this one would fail for it.
    if (adminThrough) {
      const released = await adminThrough.put(oidc, { data: { ...PROVIDER, ssoRequired: false } });
      expect.soft(released.ok(), `passwords are taken again: ${released.status()}`).toBe(true);
      await adminThrough.dispose();
    }
    const removed = await admin.delete(oidc);
    expect.soft(removed.status(), 'the provider is removed').toBe(204);
    await admin.dispose();
    await providerSignsIn({
      subject: 'oidc-person',
      email: 'oidc-person@example.test',
      preferredUsername: 'oidc-person',
      name: 'OIDC Person',
    });
  }
});
