/**
 * Changing the account's address in the settings, on the real server: asked
 * with the password, the address waits for the link mailed to the new one,
 * which a browser that is not signed in opens; then the address is the new
 * one, the old one is told so, and the account signs in by the new one only.
 */
import { expect, test } from '@playwright/test';

import { apiClient, inbox, person, register, signInOnPage, watchErrors } from './support.ts';

test.use({ locale: 'en-US' });

test('the address is changed by the link mailed to the new one, opened where nobody is signed in', async ({
  page,
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const who = person('mover');
  const moved = `${who.username}@home.example.test`;
  await register(base, who);

  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, who);
  await page.getByTestId('open-settings').click();
  const section = page.getByTestId('settings-dialog').getByTestId('email-change');
  const address = section.getByTestId('email-change-address');
  await expect(address).toHaveText(who.email);
  await section.getByRole('button', { name: 'Change address' }).click();
  const password = section.getByLabel('Current password');
  const ask = section.getByRole('button', { name: 'Change address' });
  await section.getByLabel('New email address').fill(moved);

  // Not without the account's password.
  await password.fill('not the password');
  await ask.click();
  await expect(section.getByRole('alert')).toHaveText('The current password is wrong.');
  await expect(password).toBeFocused();
  await expect(section.getByTestId('email-change-pending')).toHaveCount(0);

  await password.fill(who.password);
  await ask.click();
  const pending = section.getByTestId('email-change-pending');
  await expect(pending).toContainText(moved);
  await expect(address).toHaveText(who.email);
  await expect(section.getByLabel('New email address')).toHaveCount(0);

  // The link goes to the new address, and a newer one ends it.
  const linksTo = async (recipient: string): Promise<string[]> =>
    (await inbox(base))
      .filter((mail) => mail.recipient === recipient)
      .flatMap((mail) => mail.decoded.split(/\r?\n/))
      .filter((line) => line.startsWith(base) && line.includes('/confirm-email#'))
      .map((line) => line.trim());
  await expect.poll(async () => (await linksTo(moved)).length).toBe(1);
  await pending.getByRole('button', { name: 'Send the link again' }).click();
  await expect(section.getByRole('status')).toContainText(
    'Asked for again. A new link ends the one before it.',
  );
  await expect.poll(async () => (await linksTo(moved)).length).toBe(2);
  const [first, second] = await linksTo(moved);
  if (!first || !second) throw new Error('the links were not mailed');
  expect(new URL(second).hash).toHaveLength(44);

  // In a browser that is not signed in: the first link changes nothing, the second does.
  const elsewhere = await browser.newContext({ locale: 'en-US' });
  const visitor = await elsewhere.newPage();
  const visitorErrors = watchErrors(visitor);
  await visitor.goto(first);
  await expect(visitor.getByRole('alert')).toContainText('This link changes nothing.');
  await visitor.goto(second);
  await expect(visitor.getByTestId('confirm-email-done')).toContainText('Your address is changed.');
  expect(new URL(visitor.url()).hash).toBe('');
  // Used once: opened again, it changes nothing.
  await visitor.goto(second);
  await expect(visitor.getByRole('alert')).toContainText('This link changes nothing.');
  // Nobody was signed in by it.
  await visitor.getByRole('link', { name: 'Continue' }).click();
  await expect(visitor.getByTestId('sign-in-form')).toBeVisible();
  expect(visitorErrors).toEqual([]);
  await elsewhere.close();

  // The settings read it anew as they open: the new address, and nothing waiting.
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('settings-dialog')).toHaveCount(0);
  await page.getByTestId('open-settings').click();
  await expect(address).toHaveText(moved);
  await expect(pending).toHaveCount(0);

  // The old address is told, without the new one; and the account is the new address's.
  await expect
    .poll(
      async () =>
        (await inbox(base))
          .filter((mail) => mail.recipient === who.email)
          .filter((mail) => !mail.decoded.includes('/verify-email#')).length,
    )
    .toBe(1);
  const told = (await inbox(base)).filter((mail) => mail.recipient === who.email).at(-1);
  expect(told?.decoded).toContain(who.username);
  expect(told?.decoded).not.toContain('home.example.test');
  const anon = await apiClient(base);
  const login = (email: string) =>
    anon.post('/api/v1/login', { data: { email, password: who.password } });
  expect((await login(who.email)).status()).toBe(401);
  expect((await login(moved)).status()).toBe(204);
  await anon.dispose();
  expect(errors).toEqual([]);
});
