/**
 * The head of a board at the widths a person meets it at, on the in-memory
 * backend: a window of 1,000 by 800 with the sidebar beside the board (a pane
 * of 744) and folded to its strip (944), with the card search closed and
 * open; a window of 1,280; a phone of 390. Whatever the width, no name of a
 * view is cut: not by its own box, and not by the end of the row of tabs.
 *
 * The first board of the seed has four views of long names: "Progress
 * Tracker" (shown first), "Project Priorities", "Task Calendar", "Task
 * Overview".
 *
 * With `FVOCI_SCREENS=1` it also takes pictures of each, light and dark
 * (`target/screens/board-head/`).
 */
import path from 'node:path';

import { expect, test, type Locator, type Page } from '@playwright/test';

import { screensDir } from './screens.ts';
import { headCuts as cuts, openApp, shell, sidebar, topBar } from './shell.support.ts';

const header = (page: Page): Locator => page.getByTestId('view-header');
const tool = (page: Page, name: string): Locator => header(page).locator(`[data-tool="${name}"]`);
const tabs = (page: Page): Locator => page.getByTestId('view-tabs').getByRole('link');

async function paneWidth(page: Page): Promise<number> {
  return header(page).evaluate((element) => Math.round(element.getBoundingClientRect().width));
}

async function collapseSidebar(page: Page): Promise<void> {
  await sidebar(page).getByRole('button', { name: 'Collapse sidebar' }).click();
  await expect(shell(page)).toHaveAttribute('data-sidebar', 'closed');
}

async function openSearch(page: Page): Promise<void> {
  await tool(page, 'search').click();
  await expect(header(page).getByRole('searchbox', { name: 'Search cards' })).toBeFocused();
}

async function closeSearch(page: Page): Promise<void> {
  await header(page).getByRole('searchbox', { name: 'Search cards' }).press('Escape');
  await expect(tool(page, 'search')).toBeFocused();
}

async function picture(page: Page, name: string): Promise<void> {
  if (!process.env.FVOCI_SCREENS) return;
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: path.join(screensDir('board-head'), `${name}.png`), scale: 'css' });
}

for (const scheme of ['light', 'dark'] as const) {
  test.describe(`on the ${scheme} screen`, () => {
    test.beforeEach(async ({ page }, testInfo) => {
      test.skip(testInfo.project.name !== 'desktop', 'sets its own sizes');
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
    });

    test('a window of 1,000 cuts no name of a view, beside the sidebar or without it, searching or not', async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1000, height: 800 });
      await openApp(page);
      expect(await paneWidth(page)).toBe(744);
      await expect(header(page)).toHaveAttribute('data-layout', 'row');

      // Beside the sidebar: as many tabs as fit, the rest behind one more; the shown one is a tab.
      await expect(tabs(page).first()).toHaveAttribute('aria-current', 'page');
      const shown = await tabs(page).count();
      expect(shown).toBeGreaterThan(0);
      if (shown < 4) {
        await expect(tool(page, 'more-views')).toHaveText(`More views: ${String(4 - shown)}`);
      }
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `1000-sidebar-${scheme}`);

      // Searching, the tabs fold into one button that names the shown view.
      await openSearch(page);
      await expect(tabs(page)).toHaveCount(0);
      await expect(tool(page, 'views')).toHaveText('Progress Tracker');
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `1000-sidebar-search-${scheme}`);
      await closeSearch(page);
      await expect(tabs(page)).toHaveCount(shown);

      // The sidebar folded to its strip: more of them are tabs. (The four long names of the
      // seed are wider than its row; the welcome board's five fit: e2e-live/templates.spec.ts.)
      await collapseSidebar(page);
      expect(await paneWidth(page)).toBe(944);
      await expect.poll(() => tabs(page).count()).toBeGreaterThan(shown);
      await expect(tabs(page).first()).toHaveText('Progress Tracker');
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `1000-strip-${scheme}`);

      await openSearch(page);
      await expect(tabs(page)).toHaveCount(0);
      await expect(tool(page, 'views')).toHaveText('Progress Tracker');
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `1000-strip-search-${scheme}`);
    });

    test('while an empty search is open, the button of the views is reached and opens the list', async ({
      page,
    }) => {
      test.skip(scheme === 'dark', 'the same on either screen');
      await page.setViewportSize({ width: 1000, height: 800 });
      await openApp(page);
      const views = tool(page, 'views');

      // By Shift+Tab from the empty field: the button stays, and Enter opens its list.
      await openSearch(page);
      await page.keyboard.press('Shift+Tab');
      await expect(views).toBeFocused();
      await expect(header(page).locator('[data-testid="view-search"]')).toBeVisible();
      await page.keyboard.press('Enter');
      await expect(page.getByRole('menuitem', { name: 'Task Overview' })).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(views).toBeFocused();

      // By the pointer: the list opens where the button was pressed, and another view
      // brings the tabs back, that view's the current one.
      await header(page).getByRole('searchbox', { name: 'Search cards' }).focus();
      await views.click();
      await page.getByRole('menuitem', { name: 'Task Overview' }).click();
      await expect(tabs(page).filter({ hasText: 'Task Overview' })).toHaveAttribute(
        'aria-current',
        'page',
      );
      await expect(views).toHaveCount(0);
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
    });

    test('a window of 1,280 cuts no name, searching or not', async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await openApp(page);
      await expect(tabs(page)).toHaveCount(4);
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `1280-${scheme}`);
      await openSearch(page);
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `1280-search-${scheme}`);
    });

    test('a phone of 390 has the folded head, and nothing spills', async ({ page }) => {
      await page.setViewportSize({ width: 390, height: 844 });
      await openApp(page);
      await expect(header(page)).toHaveAttribute('data-layout', 'folded');
      await expect(tool(page, 'views')).toHaveText('Progress Tracker');
      await expect(topBar(page).getByTestId('container-description')).toBeHidden();
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `390-${scheme}`);
      await openSearch(page);
      expect(await cuts(page)).toEqual({ cut: [], scroll: 0 });
      await picture(page, `390-search-${scheme}`);
    });
  });
}
