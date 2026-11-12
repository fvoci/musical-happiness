/**
 * Accounts on the real server: making one, signing in and out. The session
 * is the server's cookie; the page never sees it.
 */
import { expect, test } from '@playwright/test';

import { boardLink } from '../e2e/shell.support.ts';

import {
  boardPath,
  invitation,
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

test('a new account starts on its own welcome board, made once, and signs out again', async ({
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

  // Signed in at once, on a copy of the welcome board of their own.
  await expect(page.getByTestId('app-shell')).toBeVisible();
  const welcome = boardLink(page, 'Welcome to fvoci!');
  await expect(welcome).toHaveAttribute('aria-current', 'page');

  // The session outlives a reload: it is the server's cookie. No second copy.
  await page.reload();
  await expect(welcome).toHaveCount(1);

  await page.getByTestId('open-settings').click();
  const settings = page.getByTestId('settings-dialog');
  await expect(settings.getByTestId('signed-in-as')).toHaveText(`Signed in as ${who.username}`);
  await settings.getByRole('button', { name: 'Sign out' }).click();

  await expect(page.getByTestId('sign-in-form')).toBeVisible();
  // Signed out on the server too: a reload asks again.
  await page.reload();
  await expect(page.getByTestId('sign-in-form')).toBeVisible();

  await signInOnPage(page, who);
  await expect(welcome).toHaveCount(1);
  expect(errors).toEqual([]);
});

test('a registration that is refused says what is wrong beside each field', async ({
  page,
  baseURL,
}) => {
  const errors = watchErrors(page);
  const who = person('careless');
  const taken = person('earlier');
  await register(baseURL ?? '', taken);
  const code = await invitation(baseURL ?? '');

  await page.goto('/');
  await page.getByRole('button', { name: /create one/i }).click();
  const form = page.getByTestId('register-form');
  const name = form.getByLabel(/^user name$/i);
  const email = form.getByLabel(/email/i);
  const password = form.getByLabel(/^password$/i);
  const invitationCode = form.getByLabel(/invitation/i);
  const submit = form.getByRole('button', { name: /^create account$/i });

  // Three fields that the browser's own check lets through and the server
  // refuses: a blank name, an address that is too long, a short password.
  await name.fill(' ');
  await email.fill(`${'a'.repeat(250)}@example.test`);
  await password.fill('short');
  await invitationCode.fill(code);
  await submit.click();
  // All three are told at once, each beside its field.
  await expect(form.getByTestId('problem-username')).toHaveText('Fill this in.');
  await expect(form.getByTestId('problem-email')).toHaveText('This is too long.');
  await expect(form.getByTestId('problem-password')).toHaveText('Use at least 8 characters.');
  await expect(form.getByRole('alert')).toHaveCount(3);
  await expect(name).toHaveAttribute('aria-invalid', 'true');
  await expect(name).toHaveAttribute('aria-describedby', 'account-username-problem');
  // The first field that is wrong is where the reader goes on.
  await expect(name).toBeFocused();
  await expect(invitationCode).not.toHaveAttribute('aria-invalid');

  // Two put right: the third is the only one still told.
  await name.fill(who.username);
  await email.fill(who.email);
  await submit.click();
  await expect(form.getByTestId('problem-password')).toHaveText('Use at least 8 characters.');
  await expect(form.getByRole('alert')).toHaveCount(1);
  await expect(name).not.toHaveAttribute('aria-invalid');
  await expect(password).toBeFocused();

  // What the server finds only when it looks things up is told beside its field too.
  await password.fill(who.password);
  await invitationCode.fill('not-the-code');
  await submit.click();
  await expect(form.getByTestId('problem-token')).toHaveText('The invitation code is wrong.');
  await expect(form.getByRole('alert')).toHaveCount(1);
  await invitationCode.fill(code);
  await name.fill(taken.username);
  await submit.click();
  await expect(form.getByTestId('problem-username')).toHaveText('That user name is taken.');
  await expect(form.getByRole('alert')).toHaveCount(1);

  // With everything right the account is made and signed in.
  await name.fill(who.username);
  await submit.click();
  await expect(page.getByTestId('app-shell')).toBeVisible();
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
  await page.goto(boardPath(board, board.viewId));
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
