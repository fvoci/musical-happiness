/**
 * The versions of a card's body, end to end, on the in-memory backend, in
 * the three stages of the page: listed from the card's menu, read, and one
 * restored for everyone; what who only reads is offered; and what is said
 * when a version cannot be restored.
 *
 * `FVOCI_SCREENS=1` also takes the pictures of the dialog, wide and 390 px
 * wide, into `target/screens/versions/` (`e2e/screens.ts`).
 */
import path from 'node:path';

import { expect, test, type Locator, type Page } from '@playwright/test';

import { boardPath } from './address.ts';
import { screensDir } from './screens.ts';

test.describe.configure({ mode: 'parallel', timeout: 90_000 });

/** What `(window as unknown as { fvociDev: Dev }).fvociDev` offers a test (src/app/backend.ts). Only what is used here. */
interface DevBackend {
  me(): Promise<{ id: string; username: string }>;
  workspaces(): Promise<{ id: string }[]>;
  boards(workspaceId: string): Promise<{ id: string; title: string }[]>;
  views(boardId: string): Promise<{ id: string; title: string }[]>;
  cards(boardId: string): Promise<{ id: string; title: string }[]>;
  card(boardId: string, cardId: string): Promise<{ updateAt: number }>;
  addMember(boardId: string, userId: string, role: 'admin'): Promise<unknown>;
  setMemberRole(boardId: string, userId: string, role: 'viewer'): Promise<unknown>;
  writeCardBody(boardId: string, cardId: string, write: object): Promise<unknown>;
  failNext(method: string, error: unknown): void;
}
interface Dev {
  backend: DevBackend;
  someoneElse(): DevBackend;
}

interface Place {
  boardId: string;
  viewId: string;
  cardId: string;
}

const paragraph = (text: string): object => ({
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
});

/** Opens a card that has a body, by its address, and waits until its editor is there. */
async function openTaskCard(page: Page): Promise<Place> {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  const place = await page.evaluate(async () => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const [workspace] = await backend.workspaces();
    const boards = await backend.boards(workspace?.id ?? '');
    const board = boards.find((each) => each.title.startsWith('Project Tasks'));
    if (!board) throw new Error('no board of tasks');
    const views = await backend.views(board.id);
    const cards = await backend.cards(board.id);
    const card = cards.find((each) => each.title === 'Identify dependencies');
    if (!card) throw new Error('no card');
    const first = [...views].sort((a, b) => a.title.localeCompare(b.title))[0];
    return { boardId: board.id, viewId: first?.id ?? '', cardId: card.id };
  });
  await page.goto(boardPath(place.boardId, place.viewId, place.cardId));
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  return place;
}

const editor = (page: Page): Locator =>
  page.getByTestId('card-body').locator('[contenteditable]').first();
const versions = (page: Page): Locator => page.getByTestId('body-versions');
const rows = (page: Page): Locator => versions(page).locator('[data-version]');
const shown = (page: Page): Locator => page.getByTestId('body-version');

async function openVersions(page: Page): Promise<void> {
  await page.getByTestId('card-menu').click();
  await page.locator('[data-action="versions"]').click();
  await expect(versions(page)).toBeVisible();
}

async function closeVersions(page: Page): Promise<void> {
  await versions(page).getByRole('button', { name: 'Close' }).click();
  await expect(versions(page)).toHaveCount(0);
  // The card under it stays.
  await expect(page.getByTestId('card-dialog')).toBeVisible();
}

/** Narrow, the body of a version takes the place of the list. */
const narrow = (page: Page): boolean => (page.viewportSize()?.width ?? 0) < 640;

/** Somebody else, made an admin of the board, writes the card's body whole. */
async function writtenWholeElsewhere(page: Page, place: Place, text: string): Promise<void> {
  await page.evaluate(
    async ({ at, body }) => {
      const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
      const other = dev.someoneElse();
      await dev.backend.addMember(at.boardId, (await other.me()).id, 'admin');
      const card = await other.card(at.boardId, at.cardId);
      await other.writeCardBody(at.boardId, at.cardId, { body, readAt: card.updateAt });
    },
    { at: place, body: paragraph(text) },
  );
}

test('the versions of a body are listed and read, and one is restored: not an undo step of the editor', async ({
  page,
}) => {
  await openTaskCard(page);
  const before = await editor(page).locator('> *').allTextContents();

  // A body nobody changed yet has no version.
  await openVersions(page);
  await expect(page.getByTestId('body-versions-empty')).toBeVisible();
  await closeVersions(page);

  await editor(page).click();
  await page.keyboard.press('Control+End');
  await page.keyboard.press('Enter');
  await page.keyboard.type('Typed now');
  await expect(editor(page)).toContainText('Typed now');

  // The body before the reader began: by nobody known.
  await openVersions(page);
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first().getByTestId('body-version-editors')).toHaveCount(0);
  await rows(page).first().click();
  await expect(shown(page)).toContainText(before[0] ?? '');
  await expect(shown(page)).not.toContainText('Typed now');
  if (narrow(page)) {
    // The list gave way: the focus is on the way back, and comes back to the version read.
    await expect(rows(page).first()).toBeHidden();
    await expect(page.getByTestId('body-versions-back')).toBeFocused();
    await page.getByTestId('body-versions-back').click();
    await expect(rows(page).first()).toBeFocused();
    // The back button goes back to the list too, and leaves the dialog open.
    await rows(page).first().click();
    await expect(shown(page)).toBeVisible();
    await page.goBack();
    await expect(rows(page).first()).toBeVisible();
    await expect(versions(page)).toBeVisible();
    await rows(page).first().click();
  } else {
    await expect(rows(page).first()).toHaveAttribute('aria-current', 'true');
  }

  // Asked first; "Cancel" restores nothing.
  await page.getByTestId('body-version-restore').click();
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Cancel' }).click();
  await expect(versions(page)).toBeVisible();
  await page.getByTestId('body-version-restore').click();
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Restore' }).click();
  await expect(versions(page)).toHaveCount(0);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  await expect(editor(page).locator('> *')).toHaveText(before);
  // What was typed had reached the room: nothing is said lost.
  await expect(page.getByTestId('body-lost')).toHaveCount(0);

  // The restore is no step of the editor's undo: undoing brings nothing typed back.
  await editor(page).click();
  await page.keyboard.press('Control+z');
  await page.keyboard.press('Control+z');
  await expect(editor(page).locator('> *')).toHaveText(before);

  // What was typed is a version, and the body restored the newest: the card has it now.
  await openVersions(page);
  await expect(rows(page)).toHaveCount(3);
  const me = await page.evaluate(
    async () => (await (window as unknown as { fvociDev: Dev }).fvociDev.backend.me()).username,
  );
  await expect(rows(page).first().getByTestId('body-version-current')).toBeVisible();
  await expect(rows(page).first().getByTestId('body-version-editors')).toContainText(me);
  await rows(page).first().click();
  await expect(page.getByTestId('body-version-is-current')).toBeVisible();
  await expect(page.getByTestId('body-version-restore')).toHaveCount(0);
  if (narrow(page)) await page.getByTestId('body-versions-back').click();
  await rows(page).nth(1).click();
  await expect(shown(page)).toContainText('Typed now');
  await expect(page.getByTestId('body-version-restore')).toBeVisible();

  // And can be gone back to.
  await page.getByTestId('body-version-restore').click();
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Restore' }).click();
  await expect(versions(page)).toHaveCount(0);
  await expect(editor(page)).toContainText('Typed now');
});

test('who only reads the board reads the versions and is not offered to restore; a refusal is said', async ({
  page,
}) => {
  const place = await openTaskCard(page);
  const before = (await editor(page).locator('> *').first().textContent()) ?? '';
  expect(before).not.toBe('');
  await writtenWholeElsewhere(page, place, 'Written whole by somebody else');
  await expect(editor(page)).toHaveText('Written whole by somebody else');

  await openVersions(page);
  await expect(rows(page)).toHaveCount(2);
  await rows(page).nth(1).click();
  await expect(page.getByTestId('body-version-restore')).toBeVisible();

  // The server refuses: the version cannot be stored any more, then is kept no more.
  for (const [status, refusal, said] of [
    [409, 'unrestorable', 'cannot be restored'],
    [404, 'gone', 'not kept any more'],
  ] as const) {
    await page.evaluate(async (refusedWith) => {
      const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
      const thrown = await backend.card('nowhere', 'nothing').catch((error: unknown) => error);
      const BackendError = (
        thrown as { constructor: new (status: number, body: object) => unknown }
      ).constructor;
      backend.failNext('bodyVersions', new BackendError(refusedWith, { detail: 'refused' }));
    }, status);
    await page.getByTestId('body-version-restore').click();
    await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Restore' }).click();
    await expect(shown(page).locator(`[data-refusal="${refusal}"]`)).toContainText(said);
    await expect(versions(page)).toBeVisible();
  }
  await expect(editor(page)).toHaveText('Written whole by somebody else');

  // Made a viewer: the version stays to read, and nothing offers to restore it.
  await page.evaluate(async ({ boardId }) => {
    const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
    await dev.someoneElse().setMemberRole(boardId, (await dev.backend.me()).id, 'viewer');
  }, place);
  await expect(page.getByTestId('body-version-restore')).toHaveCount(0);
  await expect(shown(page)).toContainText(before);
  await closeVersions(page);
  await openVersions(page);
  await expect(rows(page)).toHaveCount(2);
  await rows(page).first().click();
  await expect(shown(page)).toContainText('Written whole by somebody else');
  await expect(page.getByTestId('body-version-restore')).toHaveCount(0);
  await expect(page.getByTestId('body-version-is-current')).toHaveCount(0);
});

test.describe('pictures', () => {
  test.skip(!process.env.FVOCI_SCREENS, 'set FVOCI_SCREENS=1 to take the pictures');
  // The language the browser asks for is the app's, until the reader chooses one.
  test.use({ locale: 'ko-KR' });

  const SCREENS = screensDir('versions');
  const SIZES = [
    { name: 'wide', width: 1280, height: 800 },
    { name: 'narrow', width: 390, height: 844 },
  ] as const;

  for (const size of SIZES) {
    test(`the versions of a body, in Korean (${size.name})`, async ({ page }, testInfo) => {
      test.skip(testInfo.project.name !== 'desktop', 'the sizes are set here');
      const shot = async (name: string): Promise<void> => {
        await page.screenshot({ path: path.join(SCREENS, `${name}-${size.name}.png`) });
      };
      await page.setViewportSize(size);
      // Nothing fades or slides: a picture is not taken halfway through.
      await page.emulateMedia({ reducedMotion: 'reduce' });
      const place = await openTaskCard(page);
      await page.getByTestId('card-menu').click();
      await expect(page.locator('[data-action="versions"]')).toBeVisible();
      await shot('menu');
      await page.keyboard.press('Escape');

      await writtenWholeElsewhere(page, place, '다음 주 화요일까지 초안을 올립니다.');
      await editor(page).click();
      await page.keyboard.press('Control+End');
      await page.keyboard.type(' 검토는 목요일에 합니다.');
      // Taken by the room before the next body is written whole over it.
      await expect(page.getByTestId('card-save-state')).toHaveAttribute('data-state', 'saved');
      await writtenWholeElsewhere(page, place, '일정이 바뀌었습니다. 초안은 금요일에 올립니다.');
      await openVersions(page);
      await expect(rows(page)).toHaveCount(4);
      await page.evaluate(() => document.fonts.ready);
      await shot('list');
      await rows(page).nth(1).click();
      await expect(shown(page)).toContainText('검토는 목요일에 합니다.');
      await shot('version');
      await page.getByTestId('body-version-restore').click();
      await expect(page.getByTestId('confirm-dialog')).toBeVisible();
      await shot('restore');
    });
  }
});
