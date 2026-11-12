/**
 * The first use of a server, from nobody to two people at work on one board:
 * the first person signs up with the name of their team and makes a board,
 * and a second comes in by the link of an invitation and edits that board.
 *
 * This is the first thing that happens on the server of a run: the project
 * of these tests comes before every other (`playwright.live.config.ts`),
 * which then find the administrator made here.
 *
 * Nobody on this way reads what the server calls where they are: the words
 * of the screens are looked at, in both languages.
 */
import { expect, test, type Page } from '@playwright/test';

import { boardLink, sidebar } from '../e2e/shell.support.ts';

import {
  ADMIN,
  ORG_NAME,
  memberLink,
  person,
  placeOf,
  signedIn,
  watchErrors,
  type Schemas,
} from './support.ts';

test.describe.configure({ mode: 'serial' });
test.use({ locale: 'en-US' });

/** What a screen must not say on the way in, in either language. */
const SERVER_WORDS = /organi[sz]ation|workspace|조직|워크스페이스/i;

/** The words of the screens that were looked at, each under the name of its screen. */
const seen: { screen: string; words: string }[] = [];

async function look(page: Page, screen: string): Promise<void> {
  seen.push({ screen, words: await page.locator('body').innerText() });
}

test('a registration that is refused says what is wrong beside each field, and makes nothing', async ({
  page,
}) => {
  const errors = watchErrors(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'New here? Start a team' }).click();
  const form = page.getByTestId('register-form');
  const team = form.getByLabel(/^team name$/i);
  const name = form.getByLabel(/^user name$/i);
  const email = form.getByLabel(/email/i);
  const password = form.getByLabel(/^password$/i);
  const submit = form.getByRole('button', { name: 'Start team' });

  // Four fields that the browser's own check lets through and the server
  // refuses: blank names, an address that is too long, a short password.
  await team.fill(' ');
  await name.fill(' ');
  await email.fill(`${'a'.repeat(250)}@example.test`);
  await password.fill('short');
  await submit.click();
  // All are told at once, each beside its field.
  await expect(form.getByTestId('problem-organizationName')).toHaveText('Fill this in.');
  await expect(form.getByTestId('problem-username')).toHaveText('Fill this in.');
  await expect(form.getByTestId('problem-email')).toHaveText('This is too long.');
  await expect(form.getByTestId('problem-password')).toHaveText('Use at least 8 characters.');
  await expect(form.getByRole('alert')).toHaveCount(4);
  await expect(team).toHaveAttribute('aria-invalid', 'true');
  // The first field that is wrong is where the reader goes on.
  await expect(team).toBeFocused();
  await look(page, 'a refused registration');

  await team.fill('x'.repeat(65));
  await name.fill(ADMIN.username);
  await email.fill(ADMIN.email);
  await password.fill(ADMIN.password);
  await submit.click();
  await expect(form.getByTestId('problem-organizationName')).toHaveText(
    'A team name can be up to 64 characters.',
  );
  await expect(form.getByRole('alert')).toHaveCount(1);
  // Nothing was made: the server still takes its first account.
  await page.reload();
  await expect(page.getByRole('button', { name: 'New here? Start a team' })).toBeVisible();
  expect(errors).toEqual([]);
});

test('somebody starts a team, and a second person comes in by its link and edits the same board', async ({
  page,
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const errors = watchErrors(page);

  // The form to start with, in Korean too.
  const korean = await browser.newContext({ locale: 'ko-KR' });
  const koreanPage = await korean.newPage();
  await koreanPage.goto(base);
  await koreanPage.getByRole('button', { name: '처음인가요? 팀 시작하기' }).click();
  await expect(koreanPage.getByLabel('팀 이름')).toBeVisible();
  await look(koreanPage, 'the form that starts a team, in Korean');

  // Signing up: the name of the team, and an account.
  await page.goto('/');
  await page.getByRole('button', { name: 'New here? Start a team' }).click();
  const form = page.getByTestId('register-form');
  await form.getByLabel(/^team name$/i).fill(ORG_NAME);
  await form.getByLabel(/^user name$/i).fill(ADMIN.username);
  await form.getByLabel(/email/i).fill(ADMIN.email);
  await form.getByLabel(/^password$/i).fill(ADMIN.password);
  await look(page, 'the form that starts a team');
  await form.getByRole('button', { name: 'Start team' }).click();

  // Signed in at once, on a copy of the welcome board of their own, made once.
  await expect(page.getByTestId('app-shell')).toBeVisible();
  const welcome = boardLink(page, 'Welcome to fvoci!');
  await expect(welcome).toHaveAttribute('aria-current', 'page');
  await page.reload();
  await expect(welcome).toHaveCount(1);
  // The server has its team now: the page that signs in offers no other.
  await koreanPage.reload();
  await expect(koreanPage.getByTestId('invitation-needed')).toBeVisible();
  await look(koreanPage, 'the sign-in page of a server with its team, in Korean');

  // The first board, open to the team.
  const made = page.waitForResponse(
    (answer) => answer.url().endsWith('/api/v1/boards-and-blocks') && answer.status() === 201,
  );
  await sidebar(page).getByTestId('add-board').click();
  await page.getByTestId('template-picker').getByTestId('empty-board').click();
  await made;
  const title = page.getByTestId('top-bar').getByTestId('board-title');
  await title.fill('Launch plan');
  await title.press('Enter');
  await expect(boardLink(page, 'Launch plan')).toBeVisible();
  await page.getByTestId('share-board').click();
  const share = page.getByTestId('share-dialog');
  await share.getByTestId('share-open').click();
  await look(page, 'the first board, shared with the team');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'New card in No Status' }).click();
  await page.getByTestId('card-title').fill('Book the venue');
  await page.goBack();
  await expect(page.getByText('Book the venue')).toBeVisible();
  const { workspaceId, boardId } = placeOf(page.url());

  // The link of an invitation.
  const api = await signedIn(base, ADMIN);
  const link = await memberLink(api, workspaceId);
  const address = `${base}/invite/${link.token ?? ''}`;

  // A second person, in a browser of their own, in Korean.
  await koreanPage.goto(address);
  await expect(koreanPage.getByTestId('invite-summary')).toHaveText(
    `${ADMIN.username} 님이 ${ORG_NAME}에 초대했습니다`,
  );
  await look(koreanPage, 'the page of an invitation, in Korean');
  const second = person('second');
  const account = koreanPage.getByTestId('invite-register-form');
  await account.getByLabel('사용자 이름').fill(second.username);
  await account.getByLabel('이메일 주소').fill(second.email);
  await account.getByLabel('비밀번호').fill(second.password);
  await account.getByRole('button', { name: '계정 만들고 참여' }).click();
  await expect(koreanPage.getByTestId('app-shell')).toBeVisible();
  await look(koreanPage, 'the team as a newcomer finds it, in Korean');

  // The same board: found, joined and edited.
  await koreanPage.goto(page.url());
  await koreanPage.getByTestId('board-join').getByRole('button').click();
  await expect(koreanPage.getByTestId('board-page')).toBeVisible();
  await look(koreanPage, 'the board, in Korean');
  await koreanPage.getByText('Book the venue').click();
  await koreanPage.getByTestId('card-title').fill('Book the venue by Friday');
  await koreanPage.goBack();
  // The first person sees the change arrive.
  await expect(page.getByText('Book the venue by Friday')).toBeVisible();
  const cards = (await (
    await api.get(`/api/v1/boards/${boardId}/cards`)
  ).json()) as Schemas['Card'][];
  expect(cards.map((card) => card.title)).toEqual(['Book the venue by Friday']);

  // The page of an invitation in English, to somebody who is signed in.
  await page.goto(address);
  await expect(page.getByTestId('invite-summary')).toHaveText(
    `${ADMIN.username} invited you to ${ORG_NAME}`,
  );
  await look(page, 'the page of an invitation');

  for (const { screen, words } of seen) {
    expect(words, `${screen} says what the server calls it`).not.toMatch(SERVER_WORDS);
    expect(words.trim(), `${screen} says something`).not.toBe('');
  }
  // The tests that follow start in a team without boards for everybody: this
  // one is its members' alone from here on.
  const closed = await api.patch(`/api/v1/boards/${boardId}`, { data: { type: 'P' } });
  expect(closed.ok()).toBe(true);
  await api.dispose();
  await korean.close();
  expect(errors).toEqual([]);
});
