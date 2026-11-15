/**
 * The comments on a wiki's page and the versions of its body, end to end, on
 * the in-memory backend: written, changed, read as they were and deleted
 * below the body; somebody else's arriving as they write them; the versions
 * listed from the page's menu, read and one restored; and what a viewer and
 * a commenter are offered.
 *
 * `FVOCI_SCREENS=1` also takes the pictures of the comments and of the
 * versions, in Korean, wide and 390 px wide, into `target/screens/wiki-w5/`
 * (`e2e/screens.ts`).
 */
import path from 'node:path';

import { expect, test, type Locator, type Page } from '@playwright/test';

import { screensDir } from './screens.ts';
import { openApp } from './shell.support.ts';
import {
  goInside,
  letSomebodyElseIn,
  nameThePage,
  newWiki,
  wikiBySomebodyElse,
  wikiPath,
  wikiPlaceOf,
} from './wiki.support.ts';

test.describe.configure({ mode: 'parallel', timeout: 90_000 });

/** What `fvociDev.someoneElse()` offers these tests of a page (src/app/backend.ts). */
interface Elsewhere {
  wiki: {
    page(wikiId: string, pageId: string): Promise<{ updateAt: number }>;
    patchPage(wikiId: string, pageId: string, patch: object): Promise<unknown>;
    createComment(wikiId: string, pageId: string, body: object): Promise<{ id: string }>;
  };
}
interface Dev {
  someoneElse(): Elsewhere;
}

interface Place {
  wikiId: string;
  pageId: string;
}

const comments = (page: Page): Locator => page.getByTestId('card-comments');
const commentRows = (page: Page): Locator => comments(page).locator('[data-comment]');
const composer = (page: Page): Locator => page.getByTestId('comment-composer');
const versions = (page: Page): Locator => page.getByTestId('body-versions');
const versionRows = (page: Page): Locator => versions(page).locator('[data-version]');

/** A new wiki of the reader with one page, "Notes", shown. */
async function aPage(page: Page): Promise<Place> {
  await openApp(page);
  await newWiki(page, 'Handbook');
  await page.getByTestId('wiki-first-page').click();
  await nameThePage(page, 'Notes');
  return wikiPlaceOf(page.url());
}

/** Somebody else on the wiki writes a comment on the page. */
async function commentElsewhere(page: Page, place: Place, text: string): Promise<void> {
  await page.evaluate(
    async ({ at, said }) => {
      const other = (window as unknown as { fvociDev: Dev }).fvociDev.someoneElse();
      const body = {
        type: 'doc',
        content: [{ type: 'paragraph', content: [{ type: 'text', text: said }] }],
      };
      await other.wiki.createComment(at.wikiId, at.pageId, body);
    },
    { at: place, said: text },
  );
}

/** Somebody else writes the page's body whole, once for each text. */
async function writtenElsewhere(page: Page, place: Place, texts: string[]): Promise<void> {
  await page.evaluate(
    async ({ at, said }) => {
      const { wiki } = (window as unknown as { fvociDev: Dev }).fvociDev.someoneElse();
      for (const text of said) {
        const read = await wiki.page(at.wikiId, at.pageId);
        const body = {
          type: 'doc',
          content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
        };
        await wiki.patchPage(at.wikiId, at.pageId, { body, readAt: read.updateAt });
      }
    },
    { at: place, said: texts },
  );
}

async function writeComment(page: Page, text: string): Promise<void> {
  await composer(page).getByTestId('comment-field').fill(text);
  await composer(page).getByTestId('comment-send').click();
  await expect(commentRows(page).last()).toContainText(text);
}

async function openVersions(page: Page): Promise<void> {
  await page.getByTestId('page-menu').click();
  await page.locator('[data-action="versions"]').click();
  await expect(versions(page)).toBeVisible();
}

test('a comment is written below the page, changed, read as it was, and deleted', async ({
  page,
}) => {
  await aPage(page);
  await expect(comments(page)).toContainText('No comments yet.');
  await writeComment(page, 'A first thought');
  const row = commentRows(page).first();
  await row.locator('[data-action="edit"]').click();
  const field = row.getByRole('textbox', { name: 'Comment text' });
  await field.fill('A second thought');
  await field.press('Control+Enter');
  await expect(row).toContainText('A second thought');
  await expect(row.getByTestId('comment-edited')).toBeVisible();
  await row.locator('[data-action="history"]').click();
  await expect(row.getByTestId('comment-history')).toContainText('A first thought');
  await row.getByRole('button', { name: /^Delete the comment from/ }).click();
  await expect(commentRows(page)).toHaveCount(0);
});

test('a comment of somebody else on the page arrives as they write it', async ({ page }) => {
  const place = await aPage(page);
  await letSomebodyElseIn(page, place.wikiId);
  await commentElsewhere(page, place, 'Seen from elsewhere');
  await expect(commentRows(page)).toHaveCount(1);
  await expect(commentRows(page).first()).toContainText('Seen from elsewhere');
  // The reader is the wiki's admin: theirs is the reader's to change and delete too.
  await expect(commentRows(page).first().locator('[data-action="edit"]')).toBeVisible();
});

test('the versions of the page are listed from its menu, read, and one restored', async ({
  page,
}) => {
  const place = await aPage(page);
  await letSomebodyElseIn(page, place.wikiId);
  await writtenElsewhere(page, place, ['The first plan', 'The second plan']);
  await expect(page.getByTestId('page-body')).toContainText('The second plan');
  await openVersions(page);
  await expect(versionRows(page)).toHaveCount(2);
  await versionRows(page).nth(1).click();
  await expect(page.getByTestId('body-version')).toContainText('The first plan');
  await page.getByTestId('body-version-restore').click();
  await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Restore' }).click();
  await expect(versions(page)).toBeHidden();
  await expect(page.getByTestId('page-body')).toContainText('The first plan');
});

for (const role of ['viewer', 'commenter'] as const) {
  test(`a ${role} of the wiki reads the comments and the versions, and is offered what the role lets`, async ({
    page,
  }) => {
    await openApp(page);
    const made = await wikiBySomebodyElse(page, {
      title: 'Rules',
      type: 'P',
      page: 'Read me',
      readerRole: role,
    });
    await writtenElsewhere(page, made, ['The rules']);
    await commentElsewhere(page, made, 'Their comment');
    await goInside(page, wikiPath(made.wikiId, made.pageId));
    await expect(commentRows(page)).toHaveCount(1);
    await expect(commentRows(page).first().locator('[data-action="edit"]')).toHaveCount(0);
    await expect(composer(page)).toHaveCount(role === 'commenter' ? 1 : 0);
    if (role === 'commenter') await writeComment(page, 'My own');
    await openVersions(page);
    await expect(versionRows(page)).toHaveCount(1);
    await versionRows(page).first().click();
    await expect(page.getByTestId('body-version')).toContainText('The rules');
    await expect(page.getByTestId('body-version-restore')).toHaveCount(0);
  });
}

test.describe('pictures', () => {
  test.skip(!process.env.FVOCI_SCREENS, 'set FVOCI_SCREENS=1 to take the pictures');
  test.use({ locale: 'ko-KR' });

  const SCREENS = screensDir('wiki-w5');
  const SIZES = [
    { name: 'wide', width: 1280, height: 800 },
    { name: '390', width: 390, height: 844 },
  ] as const;

  for (const size of SIZES) {
    test(`the comments and the versions of a page, in Korean (${size.name})`, async ({
      page,
    }, testInfo) => {
      test.skip(testInfo.project.name !== 'desktop', 'the sizes are set here');
      const shot = async (name: string): Promise<void> => {
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: path.join(SCREENS, `${name}-${size.name}.png`) });
      };
      await page.setViewportSize(size);
      await page.emulateMedia({ reducedMotion: 'reduce' });
      await openApp(page);
      await newWiki(page, '팀 안내서');
      await page.getByTestId('wiki-first-page').click();
      await nameThePage(page, '회의 메모');
      const place = wikiPlaceOf(page.url());
      await letSomebodyElseIn(page, place.wikiId);
      await writtenElsewhere(page, place, [
        '다음 회의는 화요일 오전 10시입니다.',
        '다음 회의는 목요일 오후 2시로 옮겼습니다.',
      ]);
      await commentElsewhere(page, place, '목요일이면 저도 갈 수 있어요.');
      await writeComment(page, '회의실은 3층으로 잡았습니다.');
      await commentRows(page).last().scrollIntoViewIfNeeded();
      await shot('comments');
      await openVersions(page);
      await expect(versionRows(page)).toHaveCount(2);
      await versionRows(page).nth(1).click();
      await expect(page.getByTestId('body-version')).toContainText('화요일');
      await shot('versions');
    });
  }
});
