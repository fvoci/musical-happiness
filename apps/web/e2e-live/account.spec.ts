/**
 * Accounts on the real server: making one, signing in and out. The session
 * is the server's cookie; the page never sees it.
 */
import { expect, test } from '@playwright/test';

import { invitation, person, register, signInOnPage, watchErrors } from './support.ts';

test.use({ locale: 'en-US' });

test('a new account signs in to an empty workspace and signs out again', async ({
  page,
  baseURL,
}) => {
  const errors = watchErrors(page);
  const who = person('newcomer');
  const code = await invitation(baseURL ?? '');

  await page.goto('/');
  await page.getByRole('button', { name: /create one/i }).click();
  const form = page.getByTestId('register-form');
  await form.getByLabel(/^user name$/i).fill(who.username);
  await form.getByLabel(/email/i).fill(who.email);
  await form.getByLabel(/^password$/i).fill(who.password);
  await form.getByLabel(/invitation/i).fill(code);
  await form.getByRole('button', { name: /^create account$/i }).click();

  // Signed in at once, to a workspace with no boards.
  await expect(page.getByTestId('app-shell')).toBeVisible();
  await expect(page.getByText('You have no boards yet.')).toBeVisible();

  // The session outlives a reload: it is the server's cookie.
  await page.reload();
  await expect(page.getByText('You have no boards yet.')).toBeVisible();

  await page.getByTestId('open-settings').click();
  const settings = page.getByTestId('settings-dialog');
  await expect(settings.getByTestId('signed-in-as')).toHaveText(`Signed in as ${who.username}`);
  await settings.getByRole('button', { name: 'Sign out' }).click();

  await expect(page.getByTestId('sign-in-form')).toBeVisible();
  // Signed out on the server too: a reload asks again.
  await page.reload();
  await expect(page.getByTestId('sign-in-form')).toBeVisible();

  await signInOnPage(page, who);
  await expect(page.getByText('You have no boards yet.')).toBeVisible();
  expect(errors).toEqual([]);
});

test('a wrong password is told, and nothing else happens', async ({ page, baseURL }) => {
  const who = person('forgetful');
  await register(baseURL ?? '', who);

  await page.goto('/');
  const form = page.getByTestId('sign-in-form');
  await form.getByLabel(/user name or email/i).fill(who.username);
  await form.getByLabel(/^password$/i).fill('not the password');
  await form.getByRole('button', { name: /^sign in$/i }).click();
  await expect(form.getByRole('alert')).toHaveText('The name or the password is wrong.');
  await expect(page.getByTestId('app-shell')).toHaveCount(0);
});

test('an account needs the invitation once the server has one', async ({ page, baseURL }) => {
  await invitation(baseURL ?? '');
  const who = person('uninvited');

  await page.goto('/');
  await page.getByRole('button', { name: /create one/i }).click();
  const form = page.getByTestId('register-form');
  await form.getByLabel(/^user name$/i).fill(who.username);
  await form.getByLabel(/email/i).fill(who.email);
  await form.getByLabel(/^password$/i).fill(who.password);
  await form.getByRole('button', { name: /^create account$/i }).click();
  await expect(form.getByRole('alert')).toHaveText('This server needs an invitation code.');
});
