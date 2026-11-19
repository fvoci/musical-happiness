/** A second organization uses the whole real app with PostgreSQL row policies active. */
import { expect, test } from '@playwright/test';

import { boardLink, sidebar } from '../e2e/shell.support.ts';

import {
  archiveRoundTrip,
  fileUnderCategory,
  firstOrganization,
  invitedColleague,
  observe,
  restoreBoard,
  startOrganization,
} from './organizations.support.ts';
import { person, placeOf, signedIn, watchErrors, type Schemas } from './support.ts';

test.use({ locale: 'en-US' });

test('a second organization goes from signup through the full app to a shared live board', async ({
  page,
  browser,
  baseURL,
}, info) => {
  test.slow();
  const base = baseURL ?? '';
  const forbidden = await firstOrganization(base);
  const owner = person('second-founder');
  const observation = observe(page);
  const errors = watchErrors(page);
  await startOrganization(page, owner);
  await observation.screen();
  const api = await signedIn(base, owner);
  const { workspaceId } = placeOf(page.url());
  const listed = (await (
    await api.get('/api/v1/users/me/workspaces')
  ).json()) as Schemas['UserWorkspace'][];
  expect(listed.map((item) => item.id)).toEqual([workspaceId]);

  await sidebar(page).getByTestId('add-board').click();
  await observation.screen();
  await page
    .getByTestId('template-picker')
    .getByRole('button', { name: 'Use this template: Issue tracker' })
    .click();
  await expect(boardLink(page, 'Issue tracker')).toHaveAttribute('aria-current', 'page');
  const title = 'Second planning board';
  await page.getByTestId('top-bar').getByTestId('board-title').fill(title);
  await page.getByTestId('top-bar').getByTestId('board-title').press('Enter');
  await expect(boardLink(page, title)).toBeVisible();
  const address = page.url();
  const { boardId } = placeOf(address);
  await fileUnderCategory(page, api, workspaceId, title);
  await observation.screen();

  await page.getByTestId('share-board').click();
  await page.getByTestId('share-dialog').getByTestId('share-open').click();
  await page.keyboard.press('Escape');
  await page
    .getByRole('button', { name: /^New card in / })
    .first()
    .click();
  await page.getByTestId('card-title').fill('Second launch draft');
  await page.goBack();
  await expect(page.getByText('Second launch draft', { exact: true })).toBeVisible();

  await page.getByTestId('open-people').click();
  const people = page.getByTestId('people-dialog');
  await expect(people).toContainText(owner.username);
  await people.getByTestId('people-make-link').click();
  await expect(people.getByTestId('people-link')).toHaveValue(/\/invite\/[\w-]{43}$/);
  const invitation = await people.getByTestId('people-link').inputValue();
  await observation.screen();
  await page.keyboard.press('Escape');

  await restoreBoard(page, title, () => observation.screen());
  await archiveRoundTrip(page, api, workspaceId, title, info, forbidden);
  await observation.screen();

  const colleague = await invitedColleague(browser, base, invitation, address);
  const other = colleague.page;
  await other.getByText('Second launch draft', { exact: true }).click();
  await other.getByTestId('card-title').fill('Second launch ready');
  await expect(page.getByText('Second launch ready', { exact: true })).toBeVisible();
  await expect
    .poll(() =>
      observation.frames.some(
        (frame) => frame.includes('Second launch ready') && frame.includes(boardId),
      ),
    )
    .toBe(true);
  await colleague.observation.screen();
  await observation.screen();
  await observation.assertIsolated(forbidden);
  await colleague.observation.assertIsolated(forbidden);
  expect([...errors, ...colleague.errors]).toEqual([]);
  await api.dispose();
  await colleague.context.close();
});

test('an organization started on a Korean screen begins on the welcome board in Korean', async ({
  browser,
  baseURL,
}) => {
  const context = await browser.newContext({ locale: 'ko-KR', baseURL: baseURL ?? '' });
  const page = await context.newPage();
  const errors = watchErrors(page);
  const owner = person('korean-founder');
  await page.goto('/');
  await page.getByRole('button', { name: '처음인가요? 조직 시작하기' }).click();
  const form = page.getByTestId('register-form');
  await form.getByLabel('조직 이름', { exact: true }).fill('한국어 조직');
  await form.getByLabel('사용자 이름', { exact: true }).fill(owner.username);
  await form.getByLabel('이메일 주소', { exact: true }).fill(owner.email);
  await form.getByLabel('비밀번호', { exact: true }).fill(owner.password);
  await form.getByRole('button', { name: '조직 시작하기' }).click();
  await expect(page.getByTestId('app-shell')).toBeVisible();
  // The account's settings leave the language to the device: the screen's is taken.
  await expect(boardLink(page, 'fvoci 시작하기')).toHaveAttribute('aria-current', 'page');
  await expect(page.getByText('카드를 열어 보세요', { exact: true })).toBeVisible();
  expect(errors).toEqual([]);
  await context.close();
});
