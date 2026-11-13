/** Workspace administrators manage groups; their colleagues can read them. */
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { screensDir } from '../e2e/screens.ts';
import {
  ADMIN,
  person,
  register,
  signedIn,
  signInOnPage,
  uniqueName,
  watchErrors,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

test('an administrator creates a team, changes its name and people, and deletes it', async ({
  page,
  baseURL,
  browser,
}) => {
  const base = baseURL ?? '';
  const colleague = person('group-member');
  await register(base, colleague);
  const theirs = await signedIn(base, colleague);
  const member = (await (await theirs.get('/api/v1/users/me')).json()) as Schemas['User'];
  const errors = watchErrors(page);
  await page.goto('/');
  await signInOnPage(page, ADMIN);
  await page.getByTestId('open-people').click();
  const panel = page.getByTestId('teams-panel');
  const name = uniqueName('Design');
  await panel.getByRole('textbox', { name: 'Team name' }).fill(name);
  await panel.getByRole('button', { name: 'Create team' }).click();
  let row = panel.locator(`[data-team="${name}"]`);
  await row.locator('summary').click();
  await row.getByLabel('Choose a person').selectOption(member.id);
  await row.getByRole('button', { name: 'Add a person' }).click();
  await expect(row.locator(`[data-team-member="${member.id}"]`)).toBeVisible();
  if (process.env.FVOCI_SCREENS === '1') {
    const folder = screensDir('teams');
    await page.screenshot({ path: path.join(folder, 'people-wide.png') });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(folder, 'people-390.png') });
    await page.setViewportSize({ width: 1280, height: 720 });
  }
  // Ordinary members see the group and its people, without management controls.
  const other = await browser.newContext();
  const otherPage = await other.newPage();
  await otherPage.goto(base);
  await signInOnPage(otherPage, colleague);
  await otherPage.getByTestId('open-people').click();
  const readOnly = otherPage.getByTestId('teams-panel');
  await readOnly.locator(`[data-team="${name}"] summary`).click();
  await expect(readOnly.getByText(colleague.username, { exact: true })).toBeVisible();
  await expect(readOnly.getByRole('button')).toHaveCount(0);
  await other.close();
  await row.getByRole('button', { name: 'Rename', exact: true }).click();
  const renamed = `${name}-research`;
  await row.getByRole('textbox', { name: 'Team name' }).fill(renamed);
  await row.getByRole('button', { name: 'Save name' }).click();
  row = panel.locator(`[data-team="${renamed}"]`);
  await expect(row).toBeVisible();
  await row.getByRole('button', { name: `Remove ${colleague.username} from team` }).click();
  await expect(row.locator(`[data-team-member="${member.id}"]`)).toHaveCount(0);
  await row.getByRole('button', { name: 'Delete team' }).click();
  await page.getByRole('alertdialog').getByRole('button', { name: 'Delete team' }).click();
  await expect(row).toHaveCount(0);
  expect(errors).toEqual([]);
  await theirs.dispose();
});
