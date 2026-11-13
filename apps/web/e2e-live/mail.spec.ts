import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { screensDir } from '../e2e/screens.ts';
import { apiClient, person, register, signedIn, signInOnPage, watchErrors } from './support.ts';

interface Mail {
  recipient: string;
  decoded: string;
}

export async function inbox(baseURL: string): Promise<Mail[]> {
  const url = new URL(baseURL);
  url.port = String(Number(url.port) + 3);
  const response = await fetch(new URL('/messages', url));
  return (await response.json()) as Mail[];
}

export async function mailShot(page: Page, name: string): Promise<void> {
  if (process.env.FVOCI_SCREENS !== '1') return;
  const folder = screensDir('mail');
  mkdirSync(folder, { recursive: true });
  const wide = page.viewportSize() ?? { width: 1280, height: 720 };
  await page.screenshot({ path: path.join(folder, `${name}-wide.png`) });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: path.join(folder, `${name}-390.png`) });
  await page.setViewportSize(wide);
}

test('recovers by SMTP link, signs in with the new password and ends old sessions', async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error('missing live server address');
  const origin = baseURL;
  const who = person('mail-reset');
  await register(origin, who);
  const oldSession = await signedIn(origin, who);
  const errors = watchErrors(page);
  await page.goto('/');
  await page.getByRole('link', { name: 'Forgot your password?' }).click();
  await expect(page.getByTestId('request-password-form')).toBeVisible();
  await mailShot(page, 'request');
  await page.getByLabel('Email address').fill(who.email);
  await page.getByRole('button', { name: 'Send reset link' }).click();
  await expect(page.getByRole('status')).toContainText('If this email belongs to an account');
  await expect
    .poll(async () => (await inbox(origin)).filter((mail) => mail.recipient === who.email).length)
    .toBe(1);
  const mail = (await inbox(origin)).find((mail) => mail.recipient === who.email);
  if (!mail) throw new Error('reset message was not delivered');
  const link = mail.decoded
    .split(/\r?\n/)
    .find((line) => line.startsWith(origin) && line.includes('/reset-password#'));
  if (!link) throw new Error('reset message has no link');
  expect(new URL(link.trim()).hash).toHaveLength(44);
  await page.goto(link.trim());
  await expect(page.getByTestId('reset-password-form')).toBeVisible();
  await mailShot(page, 'new-password');
  await page.getByLabel('New password').fill('New password from SMTP');
  await page.getByRole('button', { name: 'Reset password', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Your password has been reset');
  expect(new URL(page.url()).hash).toBe('');
  expect((await oldSession.get('/api/v1/users/me')).status()).toBe(401);
  const anon = await apiClient(origin);
  expect((await anon.post('/api/v1/login', { data: who })).status()).toBe(401);
  await page.getByRole('link', { name: 'Back to sign in' }).click();
  await signInOnPage(page, { ...who, password: 'New password from SMTP' });
  expect(errors).toEqual([]);
  await oldSession.dispose();
  await anon.dispose();
});
