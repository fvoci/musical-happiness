/**
 * Course study meetings, shared attendance permissions, course exams, strictly
 * personal exam scores (two accounts verifying average sharing & private score
 * isolation), unjoined open-course reader isolation, and public-link isolation
 * against the real server (`e2e:live`).
 */
import { expect, test, type APIRequestContext, type Browser, type Page } from '@playwright/test';

import {
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Person,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

const coursePath = (workspaceId: string, courseId: string, query = ''): string =>
  `/w/${workspaceId}/courses/${courseId}${query}`;

async function getJson<T>(api: APIRequestContext, url: string): Promise<T> {
  const response = await api.get(url);
  expect(response.ok(), `${url}: ${await response.text()}`).toBe(true);
  return (await response.json()) as T;
}

async function postJson<T>(api: APIRequestContext, url: string, data: object): Promise<T> {
  const response = await api.post(url, { data });
  expect(response.ok(), `${url}: ${await response.text()}`).toBe(true);
  return (await response.json()) as T;
}

async function signedInPage(
  browser: Browser,
  address: string,
  who: Person,
): Promise<{ page: Page; errors: string[] }> {
  const context = await browser.newContext({ locale: 'en-US', timezoneId: 'UTC' });
  const page = await context.newPage();
  const errors = watchErrors(page);
  await page.goto(address);
  await signInOnPage(page, who);
  return { page, errors };
}

test('creates meetings & attendance, isolates two learners’ personal exam scores while sharing averages, and hides tabs on public links', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const ada = person('meet-ada');
  const bora = person('meet-bora');
  const chloe = person('meet-chloe');
  await Promise.all([register(base, ada), register(base, bora), register(base, chloe)]);
  const [adaApi, boraApi, chloeApi] = await Promise.all([
    signedIn(base, ada),
    signedIn(base, bora),
    signedIn(base, chloe),
  ]);
  const pages: Page[] = [];
  const watched: string[][] = [];

  try {
    const [adaUser, boraUser, chloeUser] = await Promise.all([
      getJson<Schemas['User']>(adaApi, '/api/v1/users/me'),
      getJson<Schemas['User']>(boraApi, '/api/v1/users/me'),
      getJson<Schemas['User']>(chloeApi, '/api/v1/users/me'),
    ]);
    const workspaces = await getJson<Schemas['UserWorkspace'][]>(
      adaApi,
      '/api/v1/users/me/workspaces',
    );
    const shared = workspaces.find((ws) => ws.kind !== 'personal');
    if (!shared) throw new Error('no shared workspace');
    const inWorkspace = `/api/v1/w/${shared.id}`;

    const course = await postJson<Schemas['Container']>(adaApi, `${inWorkspace}/courses`, {
      title: 'Live Meetings & Exams Course',
      visibility: 'open',
      minimumRole: 'viewer',
    });
    await postJson(adaApi, `${inWorkspace}/containers/${course.id}/members`, {
      userId: boraUser.id,
      role: 'viewer',
    });
    const unit1 = await postJson<Schemas['Unit']>(
      adaApi,
      `${inWorkspace}/courses/${course.id}/units`,
      { kind: 'unit', title: 'Concurrency Basics' },
    );

    const meetingsUrl = coursePath(shared.id, course.id, '?tab=meetings');
    const adaSession = await signedInPage(browser, meetingsUrl, ada);
    pages.push(adaSession.page);
    watched.push(adaSession.errors);
    const adaPage = adaSession.page;

    // Ada creates a study meeting and marks attendance for Ada and Bora.
    await expect(adaPage.getByTestId('course-meetings-tab')).toBeVisible();
    await adaPage.getByTestId('new-meeting').click();
    const meetingDialog = adaPage.getByTestId('course-meeting-dialog');
    await meetingDialog.getByTestId('meeting-title-input').fill('Live Kickoff');
    await meetingDialog.getByTestId('meeting-date-input').fill('2026-12-05');
    await meetingDialog.getByTestId('meeting-time-input').fill('18:00');
    await meetingDialog.getByTestId('meeting-location-input').fill('Room 204');
    await meetingDialog.getByTestId(`meeting-unit-option-${unit1.id}`).check();
    await meetingDialog.getByTestId('meeting-submit').click();
    await expect(meetingDialog).toBeHidden();

    const adaMeeting = adaPage.getByTestId('meeting-card').first();
    await expect(adaMeeting.getByTestId('meeting-title')).toHaveText('Live Kickoff');
    const adaOwnRow = adaMeeting.locator(
      `[data-testid="attendance-row"][data-user-id="${adaUser.id}"]`,
    );
    await adaOwnRow.getByTestId('attendance-btn-present').click();
    await expect(adaOwnRow.getByTestId('attendance-btn-present')).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    // Bora (Viewer) opens Meetings tab: cannot create meetings, sees Ada read-only, CAN mark own attendance.
    const boraSession = await signedInPage(browser, meetingsUrl, bora);
    pages.push(boraSession.page);
    watched.push(boraSession.errors);
    const boraPage = boraSession.page;

    await expect(boraPage.getByTestId('course-meetings-tab')).toBeVisible();
    await expect(boraPage.getByTestId('new-meeting')).toHaveCount(0);
    const boraMeeting = boraPage.getByTestId('meeting-card').first();
    const boraSeesAdaRow = boraMeeting.locator(
      `[data-testid="attendance-row"][data-user-id="${adaUser.id}"]`,
    );
    await expect(boraSeesAdaRow.getByTestId('attendance-readonly-status')).toHaveText('Present');
    const boraOwnRow = boraMeeting.locator(
      `[data-testid="attendance-row"][data-user-id="${boraUser.id}"]`,
    );
    await boraOwnRow.getByTestId('attendance-btn-late').click();
    await expect(boraOwnRow.getByTestId('attendance-btn-late')).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await boraPage.reload();
    await expect(
      boraPage
        .getByTestId('meeting-card')
        .first()
        .locator(`[data-testid="attendance-row"][data-user-id="${boraUser.id}"]`)
        .getByTestId('attendance-btn-late'),
    ).toHaveAttribute('aria-pressed', 'true');

    // Ada switches to Exams tab, creates an exam, and records her personal score (80).
    await adaPage.getByTestId('course-tab-exams').click();
    await expect(adaPage.getByTestId('exams-privacy-notice')).toBeVisible();
    await adaPage.getByTestId('new-exam').click();
    const examDialog = adaPage.getByTestId('course-exam-dialog');
    await examDialog.getByTestId('exam-title-input').fill('Live Midterm');
    await examDialog.getByTestId('exam-date-input').fill('2026-12-12');
    await examDialog.getByTestId('exam-max-score-input').fill('100');
    await examDialog.getByTestId('exam-submit').click();
    await expect(examDialog).toBeHidden();

    const adaExam = adaPage.getByTestId('exam-card').first();
    await adaExam.getByTestId('exam-my-score-input').fill('80');
    await adaExam.getByTestId('exam-my-note-input').fill('ADA_PRIVATE_REFLECTION');
    await adaExam.getByTestId('exam-my-score-save').click();
    await expect(adaExam.getByTestId('exam-my-score-saved')).toContainText('80 / 100');
    await expect(adaExam.getByTestId('exam-average')).toContainText('Requires 2+ other scores');
    await expect(adaExam.getByTestId('exam-score-count')).toContainText('1 submitted');

    // Bora switches to Exams tab: with only 1 scorer, average is hidden; saves own score (100) -> 2 scorers, still hidden!
    await boraPage.getByTestId('course-tab-exams').click();
    const boraExam = boraPage.getByTestId('exam-card').first();
    await expect(boraExam.getByTestId('exam-average')).toContainText('Requires 2+ other scores');
    await expect(boraExam).not.toContainText('ADA_PRIVATE_REFLECTION');
    await boraExam.getByTestId('exam-my-score-input').fill('100');
    await boraExam.getByTestId('exam-my-note-input').fill('BORA_PRIVATE_REFLECTION');
    await boraExam.getByTestId('exam-my-score-save').click();
    await expect(boraExam.getByTestId('exam-my-score-saved')).toContainText('100 / 100');
    await expect(boraExam.getByTestId('exam-average')).toContainText('Requires 2+ other scores');
    await expect(boraExam.getByTestId('exam-score-count')).toContainText('2 submitted');

    // Unjoined Chloe sees join hints, zero score inputs, and zero aggregate leak (`No scores yet` / `0 submitted`).
    const chloeSession = await signedInPage(
      browser,
      coursePath(shared.id, course.id, '?tab=exams'),
      chloe,
    );
    pages.push(chloeSession.page);
    watched.push(chloeSession.errors);
    const chloePage = chloeSession.page;
    await expect(chloePage.getByTestId('exams-join-hint')).toBeVisible();
    await expect(chloePage.getByTestId('exam-my-score-panel')).toHaveCount(0);
    await expect(chloePage.getByTestId('exam-average')).toContainText('No scores yet');
    await expect(chloePage.getByTestId('exam-score-count')).toContainText('0 submitted');

    // Enroll Chloe -> unsubmitted Chloe has 2 other scorers (Ada=80, Bora=100), so she sees 90 / 100;
    // then Chloe submits 90 (3rd scorer) -> Ada & Bora reload and see 90 / 100 with 3 submitted!
    await postJson(adaApi, `${inWorkspace}/containers/${course.id}/members`, {
      userId: chloeUser.id,
      role: 'viewer',
    });
    await chloePage.reload();
    const chloeExam = chloePage.getByTestId('exam-card').first();
    await expect(chloeExam.getByTestId('exam-average')).toContainText('90 / 100');
    await expect(chloeExam.getByTestId('exam-score-count')).toContainText('2 submitted');
    await chloeExam.getByTestId('exam-my-score-input').fill('90');
    await chloeExam.getByTestId('exam-my-note-input').fill('CHLOE_PRIVATE_REFLECTION');
    await chloeExam.getByTestId('exam-my-score-save').click();
    await expect(chloeExam.getByTestId('exam-my-score-saved')).toContainText('90 / 100');
    await expect(chloeExam.getByTestId('exam-average')).toContainText('90 / 100');
    await expect(chloeExam.getByTestId('exam-score-count')).toContainText('3 submitted');

    await adaPage.reload();
    const adaExamReloaded = adaPage.getByTestId('exam-card').first();
    await expect(adaExamReloaded.getByTestId('exam-average')).toContainText('90 / 100');
    await expect(adaExamReloaded.getByTestId('exam-score-count')).toContainText('3 submitted');
    await expect(adaExamReloaded.getByTestId('exam-my-score-saved')).toContainText('80 / 100');
    await expect(adaExamReloaded).not.toContainText('BORA_PRIVATE_REFLECTION');
    await expect(adaExamReloaded).not.toContainText('CHLOE_PRIVATE_REFLECTION');

    // Bora deletes own score -> submitted Chloe now has only 1 other scorer so average hides again,
    // while unsubmitted Bora (2 other scorers: Ada=80, Chloe=90) sees 85 / 100!
    await boraExam.getByTestId('exam-my-score-delete').click();
    await expect(boraExam.getByTestId('exam-average')).toContainText('85 / 100');
    await expect(boraExam.getByTestId('exam-score-count')).toContainText('2 submitted');
    await chloePage.reload();
    await expect(
      chloePage.getByTestId('exam-card').first().getByTestId('exam-average'),
    ).toContainText('Requires 2+ other scores');

    // Public sharing link makes zero /meetings or /exams requests and hides all course tabs.
    await adaPage.goto(coursePath(shared.id, course.id));
    await adaPage.getByTestId('share-course').click();
    const shareDialog = adaPage.getByTestId('share-dialog');
    await shareDialog.getByTestId('share-public').click();
    const sharedUrl = await shareDialog.getByTestId('share-link').inputValue();

    const publicContext = await browser.newContext({ locale: 'en-US' });
    const publicPage = await publicContext.newPage();
    pages.push(publicPage);
    watched.push(watchErrors(publicPage));
    const forbiddenRequests: string[] = [];
    publicPage.on('request', (req) => {
      if (req.url().includes('/meetings') || req.url().includes('/exams')) {
        forbiddenRequests.push(req.url());
      }
    });
    await publicPage.goto(sharedUrl);
    await expect(publicPage.getByTestId('shared-course')).toBeVisible();
    await expect(publicPage.getByTestId('unit-title')).toHaveText('Concurrency Basics');
    await expect(publicPage.getByTestId('course-tabs')).toHaveCount(0);
    await expect(publicPage.getByTestId('course-meetings-tab')).toHaveCount(0);
    await expect(publicPage.getByTestId('course-exams-tab')).toHaveCount(0);
    expect(forbiddenRequests).toEqual([]);

    expect(watched.flat()).toEqual([]);
  } finally {
    await Promise.all([
      ...pages.map((p) => p.context().close()),
      adaApi.dispose(),
      boraApi.dispose(),
    ]);
  }
});
