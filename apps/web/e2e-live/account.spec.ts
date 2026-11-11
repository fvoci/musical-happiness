/**
 * Accounts on the real server: making one, signing in and out. The session
 * is the server's cookie; the page never sees it.
 */
import { expect, test } from '@playwright/test';

import {
  invitation,
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

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

test('the email address signs in as well as the name, and a name may have an @ in it', async ({
  page,
  baseURL,
}) => {
  const byEmail = person('mailer');
  await register(baseURL ?? '', byEmail);
  await page.goto('/');
  await signInOnPage(page, { ...byEmail, username: byEmail.email });
  await expect(page.getByText('You have no boards yet.')).toBeVisible();

  // A second person, on a page of their own, whose name looks like an address.
  const context = await page.context().browser()?.newContext({ locale: 'en-US' });
  if (!context) throw new Error('no browser');
  const other = await context.newPage();
  const odd = person('at');
  await register(baseURL ?? '', { ...odd, username: `${odd.username}@home` });
  await other.goto(page.url());
  await signInOnPage(other, { ...odd, username: `${odd.username}@home` });
  await expect(other.getByText('You have no boards yet.')).toBeVisible();
  await context.close();
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

test('a session that ends on an open board, and somebody else signs in on that page, shows only theirs', async ({
  page,
  context,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('leaver');
  const next = person('arriver');
  await register(base, owner);
  await register(base, next);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Private plans',
    cards: [{ title: 'Secret step', status: 'To do' }],
  });
  await api.dispose();

  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}`);
  await signInOnPage(page, owner);
  await expect(page.getByText('Secret step')).toBeVisible();
  // Something to undo, in this session.
  await page.getByRole('button', { name: 'New card in To do' }).click();
  await page.getByTestId('card-title').fill('Another step');
  await page.goBack();
  const undo = page.getByRole('button', { name: 'Undo' });
  await expect(undo).toBeEnabled();

  // The session ends (the cookie is gone); the next request tells, and the
  // page asks to sign in.
  await context.clearCookies();
  await page.getByText('Secret step').click();
  await expect(page.getByTestId('sign-in-form')).toBeVisible();

  // Somebody else signs in on the same page: nothing of the first session stays.
  await signInOnPage(page, next);
  await expect(page.getByTestId('app-shell')).toBeVisible();
  await expect(page.getByText('Secret step')).toHaveCount(0);
  await expect(page.getByTestId('sidebar').getByText('Private plans')).toHaveCount(0);
  // Nor what the first one could undo.
  await expect(undo).toBeDisabled();
  expect(errors).toEqual([]);
});
