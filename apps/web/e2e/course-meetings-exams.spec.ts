/**
 * Course study meetings, shared attendance, course exams, strictly personal
 * exam scores, erased-user tombstones, `meeting_upcoming` notification routing,
 * public-link isolation, and 390px mobile layout on the in-memory backend.
 */
import { expect, test } from '@playwright/test';

import { WORKSPACE } from './address.ts';
import { openApp } from './shell.support.ts';
import { goInside } from './wiki.support.ts';

test.describe.configure({ mode: 'parallel', timeout: 90_000 });

interface DevCourse {
  createCourse(
    workspaceId: string,
    draft: { title: string; visibility: 'open' | 'private'; minimumRole?: 'viewer' | 'editor' },
  ): Promise<{ id: string }>;
  createUnit(
    courseId: string,
    draft: { kind?: 'unit' | 'chapter'; title?: string },
  ): Promise<{ id: string }>;
  meetings: {
    create(
      courseId: string,
      draft: {
        title: string;
        startsAt: number;
        location?: string;
        presenterId?: string | null;
        hostId?: string | null;
        unitIds?: string[];
        notes?: string;
      },
    ): Promise<{ id: string }>;
    putAttendance(
      courseId: string,
      meetingId: string,
      userId: string,
      update: { status: 'present' | 'late' | 'absent' | 'excused' },
    ): Promise<unknown>;
  };
  exams: {
    create(
      courseId: string,
      draft: { title: string; examDate?: string | null; maxScore: number; description?: string },
    ): Promise<{ id: string }>;
    putMyScore(
      courseId: string,
      examId: string,
      update: { score: number; note?: string },
    ): Promise<unknown>;
  };
}

interface Dev {
  backend: {
    userId: string;
    addMember(containerId: string, userId: string, role: string): Promise<unknown>;
    course: DevCourse;
  };
  someoneElse(index?: number): { userId: string; course: DevCourse };
  notifyUpcomingMeeting(courseId: string, meetingId: string): void;
  eraseUser(userId: string): void;
}

test('manages study meetings, shared attendance, exams, personal scores, tombstones, and meeting_upcoming notifications', async ({
  page,
}) => {
  await openApp(page);

  const ids = await page.evaluate(async (wsId) => {
    const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
    const other = dev.someoneElse(0);
    const third = dev.someoneElse(1);
    const course = await dev.backend.course.createCourse(wsId, {
      title: 'Operating Systems',
      visibility: 'open',
      minimumRole: 'viewer',
    });
    await dev.backend.addMember(course.id, other.userId, 'viewer');
    await dev.backend.addMember(course.id, third.userId, 'viewer');
    const unit = await dev.backend.course.createUnit(course.id, {
      kind: 'unit',
      title: 'Virtual Memory',
    });
    return {
      courseId: course.id,
      unitId: unit.id,
      meId: dev.backend.userId,
      otherId: other.userId,
    };
  }, WORKSPACE);

  await goInside(page, `/w/${WORKSPACE}/courses/${ids.courseId}`);
  await expect(page.getByTestId('course-tabs')).toBeVisible();
  await expect(page.getByTestId('course-tab-units')).toHaveAttribute('aria-selected', 'true');

  // Keyboard tab switch: ArrowRight -> Meetings tab.
  await page.getByTestId('course-tab-units').focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByTestId('course-tab-meetings')).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('course-meetings-tab')).toBeVisible();
  await expect(page.getByTestId('meetings-empty')).toBeVisible();

  // Verify no horizontal overflow on narrow mobile viewports (390px).
  const meetingsOverflow = await page.evaluate(
    () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
  );
  expect(meetingsOverflow).toBe(true);

  // Create a study meeting with linked unit, presenter, location URL, and notes.
  await page.getByTestId('new-meeting').click();
  const meetingDialog = page.getByTestId('course-meeting-dialog');
  await expect(meetingDialog).toBeVisible();
  await meetingDialog.getByTestId('meeting-title-input').fill('Week 1 Study Session');
  await meetingDialog.getByTestId('meeting-date-input').fill('2026-12-01');
  await meetingDialog.getByTestId('meeting-time-input').fill('19:30');
  await meetingDialog.getByTestId('meeting-location-input').fill('https://meet.example.com/os-w1');
  await meetingDialog.getByTestId('meeting-presenter-select').selectOption(ids.otherId);
  await meetingDialog.getByTestId(`meeting-unit-option-${ids.unitId}`).check();
  await meetingDialog.getByTestId('meeting-notes-input').fill('Bring page table diagrams.');
  await meetingDialog.getByTestId('meeting-submit').click();
  await expect(meetingDialog).toBeHidden();

  const card = page.getByTestId('meeting-card').first();
  await expect(card.getByTestId('meeting-title')).toHaveText('Week 1 Study Session');
  await expect(card.getByTestId('meeting-location')).toContainText(
    'https://meet.example.com/os-w1',
  );
  await expect(card.getByTestId('meeting-notes')).toContainText('Bring page table diagrams.');
  await expect(card.getByTestId('meeting-unit-chip')).toContainText('Virtual Memory');

  const meetingId = (await card.getAttribute('data-meeting-id')) ?? '';
  expect(meetingId).not.toBe('');

  // Mark attendance for both self and other member.
  const myRow = card.locator(`[data-testid="attendance-row"][data-user-id="${ids.meId}"]`);
  const otherRow = card.locator(`[data-testid="attendance-row"][data-user-id="${ids.otherId}"]`);
  await myRow.getByTestId('attendance-btn-present').click();
  await expect(myRow.getByTestId('attendance-btn-present')).toHaveAttribute('aria-pressed', 'true');
  await otherRow.getByTestId('attendance-btn-late').click();
  await expect(otherRow.getByTestId('attendance-btn-late')).toHaveAttribute('aria-pressed', 'true');
  await expect(card.getByTestId('attendance-summary-present')).toContainText('1');
  await expect(card.getByTestId('attendance-summary-late')).toContainText('1');

  // Clicking the linked unit chip switches back to the Units tab and opens the unit.
  await card.getByTestId('meeting-unit-chip').click();
  await expect(page.getByTestId('course-tab-units')).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('unit-title')).toHaveValue('Virtual Memory');

  // Switch to Exams tab -> privacy notice is visible; create exam & record personal score.
  await page.getByTestId('course-tab-exams').click();
  await expect(page.getByTestId('course-exams-tab')).toBeVisible();
  await expect(page.getByTestId('exams-privacy-notice')).toBeVisible();

  await page.getByTestId('new-exam').click();
  const examDialog = page.getByTestId('course-exam-dialog');
  await examDialog.getByTestId('exam-title-input').fill('Midterm Exam');
  await examDialog.getByTestId('exam-date-input').fill('2026-12-10');
  await examDialog.getByTestId('exam-max-score-input').fill('100');
  await examDialog.getByTestId('exam-description-input').fill('Chapters 1 to 4.');
  await examDialog.getByTestId('exam-submit').click();
  await expect(examDialog).toBeHidden();

  const examCard = page.getByTestId('exam-card').first();
  await expect(examCard.getByTestId('exam-title')).toHaveText('Midterm Exam');
  await expect(examCard.getByTestId('exam-average')).toContainText('No scores yet');

  // Client validation rejects score > maxScore.
  await examCard.getByTestId('exam-my-score-input').fill('120');
  await examCard.getByTestId('exam-my-score-save').click();
  await expect(examCard.getByTestId('exam-my-score-error')).toBeVisible();

  // k-anonymity: with 1 other score + own score (2 total, only 1 other), average stays hidden!
  const examId = (await examCard.getAttribute('data-exam-id')) ?? '';
  await page.evaluate(
    async ({ courseId, eid }) => {
      const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
      await dev.someoneElse(0).course.exams.putMyScore(courseId, eid, {
        score: 95,
        note: 'OTHER_SECRET_NOTE',
      });
    },
    { courseId: ids.courseId, eid: examId },
  );
  await examCard.getByTestId('exam-my-score-input').fill('85');
  await examCard.getByTestId('exam-my-note-input').fill('Need more practice on TLBs');
  await examCard.getByTestId('exam-my-score-save').click();
  await expect(examCard.getByTestId('exam-my-score-saved')).toContainText('85 / 100');
  await expect(examCard.getByTestId('exam-average')).toContainText('Requires 2+ other scores');
  await expect(examCard.getByTestId('exam-score-count')).toContainText('2 submitted');
  await expect(examCard).not.toContainText('OTHER_SECRET_NOTE');

  // 3rd member submits score (90) -> now 2 other scores exist, so average (90 / 100) is shown!
  await page.evaluate(
    async ({ courseId, eid }) => {
      const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
      await dev.someoneElse(1).course.exams.putMyScore(courseId, eid, {
        score: 90,
        note: 'THIRD_SECRET_NOTE',
      });
    },
    { courseId: ids.courseId, eid: examId },
  );
  await page.getByTestId('course-tab-meetings').click();
  await page.getByTestId('course-tab-exams').click();
  const refreshedCard = page.getByTestId('exam-card').first();
  await expect(refreshedCard.getByTestId('exam-average')).toContainText('90 / 100');
  await expect(refreshedCard.getByTestId('exam-score-count')).toContainText('3 submitted');
  await expect(refreshedCard).not.toContainText('THIRD_SECRET_NOTE');

  // Trigger `meeting_upcoming` notification and click it from the bell -> opens Meetings tab & focuses meeting!
  await page.evaluate(
    ({ courseId, mid }) => {
      const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
      dev.notifyUpcomingMeeting(courseId, mid);
    },
    { courseId: ids.courseId, mid: meetingId },
  );

  // Open notifications bell (or mobile bar menu on narrow viewport).
  const bell = page.getByTestId('open-notifications');
  if (await bell.isVisible()) {
    await bell.click();
  } else {
    await page.getByTestId('top-bar').getByTestId('bar-menu').click();
    await page.getByRole('menuitem', { name: /^Notifications/ }).click();
  }
  const notifItem = page
    .getByTestId('notifications')
    .getByRole('link', { name: /Upcoming study meeting: Week 1 Study Session/ });
  await expect(notifItem).toBeVisible();
  await notifItem.click();

  await expect(page).toHaveURL(new RegExp(`tab=meetings&meeting=${meetingId}`));
  await expect(page.getByTestId('course-meetings-tab')).toBeVisible();
  await expect(
    page.locator(`[data-testid="meeting-card"][data-meeting-id="${meetingId}"]`),
  ).toHaveAttribute('data-focused', 'true');

  // Erasing the other user preserves meeting presenter & attendance as "Deleted user".
  await page.evaluate((uid) => {
    const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
    dev.eraseUser(uid);
  }, ids.otherId);
  await goInside(page, `/w/${WORKSPACE}/courses/${ids.courseId}?tab=meetings`);
  const reloadedCard = page.getByTestId('meeting-card').first();
  await expect(reloadedCard.getByTestId('meeting-presenter')).toContainText('Deleted user');
  const tombstoneRow = reloadedCard.locator(
    `[data-testid="attendance-row"][data-user-id="${ids.otherId}"]`,
  );
  await expect(tombstoneRow.getByTestId('attendance-member-name')).toHaveText('Deleted user');
  await expect(tombstoneRow.getByTestId('attendance-readonly-status')).toHaveText('Late');
});

test('isolates unjoined open-course readers and strips all meetings/exams UI & requests on public links', async ({
  page,
}) => {
  await openApp(page);

  const courseId = await page.evaluate(async (wsId) => {
    const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
    const other = dev.someoneElse();
    const course = await other.course.createCourse(wsId, {
      title: 'Open Linear Algebra',
      visibility: 'open',
      minimumRole: 'viewer',
    });
    await other.course.createUnit(course.id, { kind: 'unit', title: 'Eigenvalues' });
    await other.course.meetings.create(course.id, {
      title: 'Open Review',
      startsAt: Date.UTC(2026, 11, 2, 14, 0, 0),
    });
    const exam = await other.course.exams.create(course.id, {
      title: 'Quiz 1',
      maxScore: 20,
    });
    await other.course.exams.putMyScore(course.id, exam.id, {
      score: 18,
      note: 'PRIVATE_CREATOR_NOTE',
    });
    return course.id;
  }, WORKSPACE);

  // Unjoined reader sees meetings read-only + join hint, and exams average without score input or creator's note.
  await goInside(page, `/w/${WORKSPACE}/courses/${courseId}?tab=meetings`);
  await expect(page.getByTestId('meetings-join-hint')).toBeVisible();
  await expect(page.getByTestId('new-meeting')).toHaveCount(0);
  await expect(page.getByTestId('attendance-btn-present')).toHaveCount(0);

  await page.getByTestId('course-tab-exams').click();
  await expect(page.getByTestId('exams-join-hint')).toBeVisible();
  await expect(page.getByTestId('exam-my-score-panel')).toHaveCount(0);
  await expect(page.getByTestId('exam-average')).toContainText('No scores yet');
  await expect(page.getByTestId('exam-score-count')).toContainText('0 submitted');
  await expect(page.getByTestId('course-exams-tab')).not.toContainText('18 / 20');
  await expect(page.getByTestId('course-exams-tab')).not.toContainText('PRIVATE_CREATOR_NOTE');

  // Public shared link makes zero /meetings or /exams requests and shows zero tabs.
  const sharedCourseId = '01930000-0000-7000-8000-00000000dd01';
  const sharedUnitId = '01930000-0000-7000-8000-00000000dd02';
  const forbiddenCalls: string[] = [];
  await page.route('**/api/v1/**', async (route) => {
    const pathname = new URL(route.request().url()).pathname;
    if (pathname.includes('/meetings') || pathname.includes('/exams')) {
      forbiddenCalls.push(pathname);
      await route.fulfill({ status: 500, json: { detail: 'forbidden on shared link' } });
      return;
    }
    if (pathname === `/api/v1/shared/${sharedCourseId}`) {
      await route.fulfill({
        json: {
          id: sharedCourseId,
          workspaceId: '',
          kind: 'course',
          title: 'Public Shared Course',
          visibility: 'private',
          icon: '',
          description: '',
          cardProperties: [],
          properties: {},
          createdBy: '',
          modifiedBy: '',
          createAt: 1,
          updateAt: 1,
          deleteAt: 0,
        },
      });
      return;
    }
    if (pathname === `/api/v1/shared/${sharedCourseId}/units`) {
      await route.fulfill({
        json: [
          {
            id: sharedUnitId,
            kind: 'unit',
            parentId: '',
            title: 'Public Unit',
            order: 'a',
            createAt: 1,
            updateAt: 1,
          },
        ],
      });
      return;
    }
    if (pathname === `/api/v1/shared/${sharedCourseId}/units/${sharedUnitId}`) {
      await route.fulfill({
        json: {
          id: sharedUnitId,
          courseId: sharedCourseId,
          kind: 'unit',
          parentId: '',
          title: 'Public Unit',
          order: 'a',
          body: null,
          createdBy: '',
          modifiedBy: '',
          createAt: 1,
          updateAt: 1,
        },
      });
      return;
    }
    await route.fulfill({ status: 404, json: { detail: 'not found' } });
  });

  await page.goto(`/shared/${sharedCourseId}?r=tok`);
  await expect(page.getByTestId('shared-course')).toBeVisible();
  await expect(page.getByTestId('unit-title')).toHaveText('Public Unit');
  await expect(page.getByTestId('course-tabs')).toHaveCount(0);
  await expect(page.getByTestId('course-meetings-tab')).toHaveCount(0);
  await expect(page.getByTestId('course-exams-tab')).toHaveCount(0);
  expect(forbiddenCalls).toEqual([]);
});
