/**
 * A server that confirms the address of a registration (`email_verification`,
 * `tools/live-server.ts --verifying`): registering makes nothing yet and says
 * to check the mail; a sign-in before the link is refused with a way to the
 * link; the link opens the account's first board, signed in; a link that was
 * replaced confirms nothing. Screens of each with `FVOCI_SCREENS=1`.
 */
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { screensDir } from '../e2e/screens.ts';
import { person, watchErrors } from './support.ts';

/** What the server's fake SMTP received, from its inbox three ports up. */
async function inbox(baseURL: string): Promise<{ recipient: string; decoded: string }[]> {
  const url = new URL(baseURL);
  url.port = String(Number(url.port) + 3);
  const response = await fetch(new URL('/messages', url));
  return (await response.json()) as { recipient: string; decoded: string }[];
}

/** The screen as it is, wide and at 390 pixels, under `screens/verify/`. */
async function verifyShot(page: Page, name: string): Promise<void> {
  if (process.env.FVOCI_SCREENS !== '1') return;
  const folder = screensDir('verify');
  mkdirSync(folder, { recursive: true });
  const wide = page.viewportSize() ?? { width: 1280, height: 720 };
  await page.screenshot({ path: path.join(folder, `${name}-wide.png`), animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  // Nothing wider than the screen: no page that scrolls sideways.
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await page.screenshot({ path: path.join(folder, `${name}-390.png`), animations: 'disabled' });
  await page.setViewportSize(wide);
}

/** The confirmation links mailed to `email` so far, oldest first. */
async function links(baseURL: string, email: string): Promise<string[]> {
  return (await inbox(baseURL))
    .filter((mail) => mail.recipient === email)
    .flatMap((mail) =>
      mail.decoded
        .split(/\r?\n/)
        .filter((line) => line.startsWith(baseURL) && line.includes('/verify-email#'))
        .map((line) => line.trim()),
    );
}

test('a registration waits for its mailed link, which signs in on the first board', async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error('missing live server address');
  const who = person('verify');
  const errors = watchErrors(page);
  await page.goto('/');
  const options = await page.request.get('/api/v1/login-options');
  expect((await options.json()) as { emailVerification: boolean }).toMatchObject({
    emailVerification: true,
  });

  await page.getByRole('button', { name: 'New here? Start an organization' }).click();
  await page.getByLabel('Organization name').fill('Verified Works');
  await page.getByLabel('User name', { exact: true }).fill(who.username);
  await page.getByLabel('Email address').fill(who.email);
  await page.getByLabel('Password', { exact: true }).fill(who.password);
  await page.getByRole('button', { name: 'Start organization' }).click();
  await expect(page.getByRole('heading', { name: 'Check your email' })).toBeFocused();
  await expect(page.getByTestId('check-mail')).toContainText(who.email);
  await expect.poll(async () => (await links(baseURL, who.email)).length).toBe(1);
  await verifyShot(page, 'check-mail');

  // Before the link: the right password is told to confirm, and mails the link again.
  await page.goto('/');
  await page.getByLabel('User name or email').fill(who.username);
  await page.getByLabel('Password', { exact: true }).fill(who.password);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Confirm your email address first');
  await page.getByRole('button', { name: 'Send the link again' }).click();
  await expect(page.getByTestId('resend-status')).toContainText('a new link is on its way');
  await verifyShot(page, 'sign-in-refused');
  await expect.poll(async () => (await links(baseURL, who.email)).length).toBe(2);
  // A wrong password is told nothing of the registration.
  await page.getByLabel('Password', { exact: true }).fill('not the password');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('The name or the password is wrong');

  const [first, second] = await links(baseURL, who.email);
  if (!first || !second) throw new Error('two links were mailed');
  // The first link was replaced by the second: it confirms nothing, and says so.
  await page.goto(first);
  await expect(page.getByTestId('verify-problem')).toContainText('replaced by a newer one');
  await expect(page.getByTestId('verify-request-form')).toBeVisible();
  await verifyShot(page, 'link-refused');

  await page.goto(second);
  await expect(page).toHaveURL(/\/w\/[^/]+\/boards\/[^/]+/);
  expect(new URL(page.url()).hash).toBe('');
  await expect(page.getByTestId('sign-in-form')).toHaveCount(0);
  const me = await page.request.get('/api/v1/users/me');
  expect(me.status()).toBe(200);
  expect(((await me.json()) as { username: string }).username).toBe(who.username);
  await verifyShot(page, 'first-board');
  expect(errors).toEqual([]);
});
