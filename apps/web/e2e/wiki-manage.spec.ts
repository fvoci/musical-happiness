/**
 * Looking after a wiki in the world in memory: deleting a page with those
 * below it, the wiki's own name, description and icon, deleting the wiki and
 * bringing it back from the trash, reading an open wiki before joining it and
 * writing in it after, sharing a wiki from its head, reading one the
 * reader may not change, the tree with the keyboard alone, and the drawer
 * of a phone's width.
 */
import { expect, test, type Page } from '@playwright/test';

import { screensDir } from './screens.ts';
import { hideSidebar, openApp, showSidebar, sidebar } from './shell.support.ts';
import {
  goInside,
  hideTree,
  nameThePage,
  newWiki,
  ON_A_PAGE,
  ON_A_WIKI,
  pageAction,
  pageBody,
  showTree,
  treeItem,
  treeShape,
  wikiBySomebodyElse,
  wikiPath,
  wikiPlaceOf,
  wikiScreen,
} from './wiki.support.ts';

/** A wiki with "Welcome", "Getting started" below it and "Policies" after it; on "Policies". */
async function wikiWithPages(page: Page): Promise<string> {
  const wikiId = await newWiki(page, 'Handbook');
  await page.getByTestId('wiki-first-page').click();
  await nameThePage(page, 'Welcome');
  await pageAction(page, 'Welcome', 'add-child');
  await nameThePage(page, 'Getting started');
  await pageAction(page, 'Welcome', 'add-after');
  await nameThePage(page, 'Policies');
  return wikiId;
}

test.describe('a wiki looked after', () => {
  test('deleting a page asks first, says those below go too, and takes them along', async ({
    page,
  }) => {
    await openApp(page);
    await wikiWithPages(page);
    await pageAction(page, 'Welcome', 'delete');
    const asked = page.getByTestId('confirm-dialog');
    await expect(asked).toContainText('“Welcome”');
    await expect(asked).toContainText('(1)');
    // Cancelled, nothing is gone.
    await asked.getByRole('button', { name: 'Cancel' }).click();
    await showTree(page);
    expect(await treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);

    await pageAction(page, 'Welcome', 'delete');
    await asked.getByRole('button', { name: 'Delete' }).click();
    await showTree(page);
    expect(await treeShape(page)).toEqual(['Policies']);
    // The focus is in the tree, where the deleted page's row was.
    await expect(treeItem(page, 'Policies')).toBeFocused();
    await expect(page.getByTestId('page-title')).toHaveValue('Policies');
  });

  test('the page shown deleted, the first page left is shown', async ({ page }) => {
    await openApp(page);
    await wikiWithPages(page);
    await pageAction(page, 'Policies', 'delete');
    await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Delete' }).click();
    await expect(page.getByTestId('page-title')).toHaveValue('Welcome');
    await expect(page.getByTestId('wiki-notice')).toHaveCount(0);
  });

  test('the wiki’s name, description and icon are changed, undone, and the wiki deleted and restored', async ({
    page,
  }) => {
    await openApp(page);
    const wikiId = await wikiWithPages(page);
    await showTree(page);
    await page.getByTestId('wiki-menu').click();
    await page.locator('[data-action="edit-wiki"]').click();
    const settings = page.getByTestId('wiki-settings');
    await settings.getByTestId('wiki-settings-name').fill('Guide');
    await settings.getByTestId('wiki-settings-description').fill('How we work.');
    await settings.getByTestId('wiki-settings-random-icon').click();
    await settings.getByTestId('wiki-settings-save').click();
    await expect(page.getByTestId('wiki-header-title')).toHaveText('Guide');
    await expect(page.getByTestId('wiki-header-description')).toHaveText('How we work.');
    await expect(page.getByTestId('wiki-header-icon')).not.toBeEmpty();
    await showSidebar(page);
    await expect(sidebar(page).getByRole('link', { name: 'Guide, wiki' })).toBeVisible();
    await hideSidebar(page);

    await page.getByTestId('undo').click();
    await showSidebar(page);
    await expect(sidebar(page).getByRole('link', { name: 'Handbook, wiki' })).toBeVisible();
    await hideSidebar(page);
    await showTree(page);
    await expect(page.getByTestId('wiki-header-description')).toHaveCount(0);
    await expect(page.getByTestId('wiki-header-icon')).toHaveCount(0);

    // Deleted from its own menu: to the first of the sidebar, and back from the trash.
    await showTree(page);
    await page.getByTestId('wiki-menu').click();
    await page.locator('[data-action="delete-wiki"]').click();
    await page.getByTestId('confirm-dialog').getByRole('button', { name: 'Delete' }).click();
    await expect(wikiScreen(page)).toHaveCount(0);
    await showSidebar(page);
    await expect(sidebar(page).getByRole('link', { name: 'Handbook, wiki' })).toHaveCount(0);
    await sidebar(page).getByRole('button', { name: 'Trash' }).click();
    const trash = page.getByTestId('board-trash');
    const item = trash.getByTestId('trash-items').locator('li', { hasText: 'Handbook' });
    await expect(item.getByTestId('trash-kind')).toHaveText('wiki');
    await item.getByRole('button', { name: /^Restore/ }).click();
    await page.keyboard.press('Escape');
    await showSidebar(page);
    await sidebar(page).getByRole('link', { name: 'Handbook, wiki' }).click();
    await expect(page).toHaveURL(new RegExp(`${wikiPath(wikiId)}/`));
    await showTree(page);
    expect(await treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies']);
  });

  test('an open wiki somebody else made is read before it is joined, and written once joined', async ({
    page,
  }) => {
    await openApp(page);
    const made = await wikiBySomebodyElse(page, {
      title: 'Design notes',
      type: 'O',
      page: 'Hello',
    });
    await goInside(page, wikiPath(made.wikiId));
    await expect(page).toHaveURL(wikiPath(made.wikiId, made.pageId));
    await expect(page.locator('h1[data-testid="page-title"]')).toHaveText('Hello');
    await expect(page.getByTestId('page-add-icon')).toHaveCount(0);
    const join = page.getByTestId('wiki-join');
    await expect(join).toContainText('Join it to write in it');
    // Its members are there to see, not to change.
    await showTree(page);
    await wikiScreen(page).getByTestId('share-wiki').click();
    const dialog = page.getByTestId('share-dialog');
    await expect(dialog.getByTestId('share-members').locator('li')).toHaveCount(1);
    await expect(dialog.getByTestId('share-open')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await join.getByRole('button').click();
    await expect(join).toHaveCount(0);
    await expect(page.getByTestId('page-title')).toHaveValue('Hello');
    await expect(wikiScreen(page)).toBeFocused();
    // Its page is now the reader's to write: the body's room let them in to write.
    await hideTree(page);
    const body = pageBody(page);
    await expect(body).toHaveAttribute('contenteditable', 'true');
    await body.click();
    await page.keyboard.type('Written after joining');
    await expect(body).toContainText('Written after joining');
  });

  test('a wiki is shared from its head: closed and opened to the workspace, and a member added', async ({
    page,
  }) => {
    await openApp(page);
    await newWiki(page, 'Shared notes');
    await showTree(page);
    await wikiScreen(page).getByTestId('share-wiki').click();
    const dialog = page.getByTestId('share-dialog');
    await expect(dialog.getByRole('heading', { name: 'Share wiki' })).toBeVisible();
    // Made open, as "New wiki" makes it; closed and opened again.
    const open = dialog.getByTestId('share-open');
    await expect(dialog).toContainText('can read this wiki and join it');
    await expect(open).toBeChecked();
    await open.click();
    await expect(open).not.toBeChecked();
    await open.click();
    await expect(open).toBeChecked();
    await dialog.getByTestId('share-candidates').getByRole('button').first().click();
    await expect(dialog.getByTestId('share-members').locator('li')).toHaveCount(2);
    // A wiki's public link is not offered: it would open nothing.
    await expect(dialog.getByTestId('share-public')).toHaveCount(0);
  });

  for (const role of ['viewer', 'commenter']) {
    test(`a ${role} of a wiki sees its pages and no way to change them`, async ({ page }) => {
      await openApp(page);
      const made = await wikiBySomebodyElse(page, {
        title: 'Rules',
        type: 'P',
        page: 'Read me',
        readerRole: role,
      });
      await goInside(page, wikiPath(made.wikiId, made.pageId));
      await expect(page.locator('h1[data-testid="page-title"]')).toHaveText('Read me');
      await expect(page.getByTestId('page-add-icon')).toHaveCount(0);
      const body = page.getByTestId('page-body');
      await expect(body).toHaveAttribute('data-editor', 'reading');
      // The editor's placeholder, drawn from its attribute, says the page is empty.
      await expect(body.locator('[data-placeholder]').first()).toHaveAttribute(
        'data-placeholder',
        'There is nothing on this page yet.',
      );
      await expect(body).toContainText('You can read this page but not change it.');
      await showTree(page);
      await expect(page.getByTestId('new-page')).toHaveCount(0);
      await expect(page.getByTestId('wiki-menu')).toHaveCount(0);
      await expect(treeItem(page, 'Read me').getByRole('button', { name: /^Menu of/ })).toHaveCount(
        0,
      );
    });
  }
});

test.describe('the tree with the keyboard alone', () => {
  test('moves, folds, opens and shows pages, and makes one inside a page', async ({ page }) => {
    await openApp(page);
    await wikiWithPages(page);
    await showTree(page);
    // Tab from the button before the tree reaches one page of it: the page shown.
    await page.getByTestId('new-page').focus();
    await page.keyboard.press('Tab');
    const welcome = treeItem(page, 'Welcome');
    const started = treeItem(page, 'Getting started');
    const policies = treeItem(page, 'Policies');
    await expect(policies).toBeFocused();
    await page.keyboard.press('Home');
    await expect(welcome).toBeFocused();
    // Left folds an open page, right opens it again and stays, then goes into it.
    await page.keyboard.press('ArrowLeft');
    await expect(welcome).toHaveAttribute('aria-expanded', 'false');
    await expect(started).toHaveCount(0);
    await page.keyboard.press('ArrowRight');
    await expect(welcome).toHaveAttribute('aria-expanded', 'true');
    await expect(welcome).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(started).toBeFocused();
    // Left from a page without children goes to its parent; down and up go row by row.
    await page.keyboard.press('ArrowLeft');
    await expect(welcome).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(started).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(policies).toBeFocused();
    await page.keyboard.press('ArrowUp');
    await expect(started).toBeFocused();
    await page.keyboard.press('End');
    await expect(policies).toBeFocused();
    await page.keyboard.press('Home');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('page-title')).toHaveValue('Welcome');

    // The menu from the keyboard: a page inside "Policies", named, and the tree has it.
    await showTree(page);
    await policies.focus();
    await page.keyboard.press('Shift+F10');
    await expect(page.locator('[data-action="add-child"]')).toBeVisible();
    await page.locator('[data-action="add-child"]').press('Enter');
    await nameThePage(page, 'Leave');
    await showTree(page);
    expect(await treeShape(page)).toEqual(['Welcome', '  Getting started', 'Policies', '  Leave']);
    await expect(page).toHaveURL(ON_A_PAGE);
    expect(wikiPlaceOf(page.url()).pageId).not.toBe('');
  });
});

test.describe('at a phone’s width', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

  test('the tree is a drawer: opened, a page made and chosen in it, and put away', async ({
    page,
  }) => {
    await openApp(page);
    await wikiWithPages(page);
    const drawer = page.getByRole('dialog', { name: 'Pages' });
    await expect(drawer).toBeHidden();
    await page.getByTestId('open-page-tree').click();
    await expect(drawer).toBeVisible();
    // Once it has slid in, nothing of it is past the edge of the screen.
    await expect
      .poll(async () => {
        const box = await drawer.boundingBox();
        return box !== null && box.x >= 0 && box.x + box.width <= 390;
      })
      .toBe(true);
    await treeItem(page, 'Welcome').click();
    await expect(drawer).toBeHidden();
    await expect(page.getByTestId('page-title')).toHaveValue('Welcome');
    await expect(pageBody(page)).toBeVisible();

    // A page made in the drawer: the drawer goes, the new page's title has the caret.
    await page.getByTestId('open-page-tree').click();
    await page.getByTestId('new-page').click();
    await expect(drawer).toBeHidden();
    await nameThePage(page, 'From the drawer');
    await page.getByTestId('open-page-tree').click();
    await expect(treeItem(page, 'From the drawer')).toBeVisible();
    await page.keyboard.press('Escape');

    await page.getByTestId('open-page-tree').click();
    await page.getByTestId('page-tree-scrim').click({ position: { x: 380, y: 400 } });
    await expect(drawer).toBeHidden();
    await hideTree(page);
  });
});

test.describe('screens', () => {
  test.skip(!process.env.FVOCI_SCREENS, 'pictures are taken only when asked for');
  test.use({ locale: 'ko-KR' });

  /** Opens a page's menu in the tree whatever the language, and presses one of its lines. */
  async function onPage(page: Page, name: string, action: string): Promise<void> {
    await showTree(page);
    // The row's last button is its menu's (a sheet in a narrow pane, a menu elsewhere).
    await treeItem(page, name).getByRole('button').last().click();
    await page.locator(`[data-action="${action}"]`).click();
  }

  for (const [width, height, size] of [
    [1280, 800, 'wide'],
    [390, 844, '390'],
  ] as const) {
    test(`the wiki in Korean, ${size}`, async ({ page }, info) => {
      test.skip(info.project.name !== 'desktop', 'one set of pictures');
      const shot = async (name: string): Promise<void> => {
        // A drawer of the tree that is out has slid all the way in first.
        const drawer = page.getByRole('dialog', { name: '페이지' });
        if (await drawer.isVisible()) {
          await expect.poll(async () => (await drawer.boundingBox())?.x).toBe(0);
        }
        await page.screenshot({ path: `${screensDir('wiki')}/${name}-${size}.png` });
      };
      await page.setViewportSize({ width, height });
      await openApp(page);

      // "New wiki", as it asks.
      await showSidebar(page);
      await sidebar(page).getByTestId('add-board').click();
      await page.getByTestId('template-picker').getByTestId('new-wiki').click();
      await page.getByTestId('new-wiki-title').fill('팀 안내서');
      await shot('new-wiki-dialog');
      await page.getByTestId('new-wiki-create').click();
      await expect(page).toHaveURL(ON_A_WIKI);
      await expect(page.getByTestId('wiki-empty')).toBeVisible();
      // The sidebar's drawer, which made the wiki, has slid all the way out.
      if (size === '390') await expect(sidebar(page)).toBeHidden();
      await shot('empty-wiki');

      // Pages three deep, a body, a viewer.
      await page.getByTestId('wiki-first-page').click();
      await nameThePage(page, '환영합니다');
      await page.getByTestId('page-add-icon').click();
      await onPage(page, '환영합니다', 'add-child');
      await nameThePage(page, '처음 오신 분께');
      await onPage(page, '처음 오신 분께', 'add-child');
      await nameThePage(page, '계정 만들기');
      await onPage(page, '환영합니다', 'add-after');
      await nameThePage(page, '일하는 방식');
      await onPage(page, '환영합니다', 'add-after');
      await nameThePage(page, '휴가와 근무 시간에 대한 아주 긴 제목의 페이지');
      await showTree(page);
      await treeItem(page, '계정 만들기').click();
      await hideTree(page);
      await pageBody(page).click();
      await page.keyboard.type(
        '계정은 첫날 받는 메일의 링크로 만듭니다. 비밀번호는 열두 자 이상으로 정하세요.',
      );
      await expect(page.getByTestId('page-body')).toHaveAttribute('data-editor', 'ready');
      await showTree(page);
      await shot('wiki-screen');

      // The page itself, the tree put away where it is a drawer.
      await hideTree(page);
      await shot('page');

      // The sidebar, boards and wikis in one list.
      await hideTree(page);
      await showSidebar(page);
      await shot('sidebar');
      await hideSidebar(page);

      // Its people, from the head of the tree.
      await showTree(page);
      await wikiScreen(page).getByTestId('share-wiki').click();
      await expect(page.getByTestId('share-dialog')).toBeVisible();
      await shot('share');
      await page.keyboard.press('Escape');
      await expect(page.getByTestId('share-dialog')).toHaveCount(0);

      // An open wiki of somebody else's, read before it is joined.
      const made = await wikiBySomebodyElse(page, { title: '디자인 노트', type: 'O', page: '색' });
      await goInside(page, wikiPath(made.wikiId));
      await expect(page.getByTestId('wiki-join')).toBeVisible();
      // The sidebar's drawer has slid all the way out.
      if (size === '390') await expect(sidebar(page)).toBeHidden();
      await shot('join');
    });
  }
});
