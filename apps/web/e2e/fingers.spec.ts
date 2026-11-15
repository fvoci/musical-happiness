/**
 * A finger on a large touch screen: an iPad held sideways (1180 by 820, the
 * Safari of an iPad Air 11 with its bars folded), where the layout is the
 * widest one yet everything is pressed with a finger.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { boardPath } from './address.ts';
import { openApp, sidebar } from './shell.support.ts';
import { newWiki, pageTree } from './wiki.support.ts';

test.use({ viewport: { width: 1180, height: 820 }, hasTouch: true });

/** A finger's 44 pixels, in both directions. */
async function expectFingerSized(target: Locator, name: string): Promise<void> {
  const box = await target.boundingBox();
  expect(box?.width, name).toBeGreaterThanOrEqual(44);
  expect(box?.height, name).toBeGreaterThanOrEqual(44);
}

/** The page itself does not scroll: every part of the layout scrolls inside its own box. */
async function expectPageStill(page: Page): Promise<void> {
  const page_ = await page.evaluate(() => ({
    wide: document.documentElement.scrollWidth - innerWidth,
    tall: document.documentElement.scrollHeight - innerHeight,
  }));
  expect(page_).toEqual({ wide: 0, tall: 0 });
}

test('the board: the page does not scroll, and what the bar offers a finger is 44 pixels', async ({
  page,
}) => {
  await openApp(page);
  await expect(page.getByTestId('app-shell')).toHaveAttribute('data-stage', 'expanded');
  await expect(sidebar(page)).toBeVisible();
  // The list of boards is longer than the screen with a finger's rows; it scrolls in itself.
  await expect
    .poll(() =>
      page.getByTestId('sidebar-list').evaluate((list) => list.scrollHeight > list.clientHeight),
    )
    .toBe(true);
  await expectPageStill(page);
  for (const id of ['share-board', 'board-presence', 'board-title']) {
    await expectFingerSized(page.getByTestId(id), id);
  }
});

test('a card: its title and the boxes of its tasks are a finger wide', async ({ page }) => {
  await openApp(page);
  const place = await page.evaluate(async () => {
    type Backend = {
      workspaces(): Promise<{ id: string }[]>;
      boards(id: string): Promise<{ id: string; title: string }[]>;
      views(id: string): Promise<{ id: string }[]>;
      cards(id: string): Promise<{ id: string; title: string }[]>;
    };
    const { backend } = (window as unknown as { fvociDev: { backend: Backend } }).fvociDev;
    const [workspace] = await backend.workspaces();
    const board = (await backend.boards(workspace?.id ?? '')).find(
      (each) => each.title === 'Project Tasks',
    );
    if (!board) throw new Error('no board of tasks');
    const [view] = await backend.views(board.id);
    const card = (await backend.cards(board.id)).find(
      (each) => each.title === 'Identify dependencies',
    );
    if (!card) throw new Error('no card');
    return { boardId: board.id, viewId: view?.id ?? '', cardId: card.id };
  });
  await page.goto(boardPath(place.boardId, place.viewId, place.cardId));
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  await expectPageStill(page);
  const title = page.getByTestId('card-title');
  expect((await title.boundingBox())?.height).toBeGreaterThanOrEqual(44);

  // The box of a task is drawn small; around it, a finger's square answers to it and to no other.
  const box = page.getByTestId('card-body').locator('ul[data-type="taskList"] input').nth(1);
  await box.scrollIntoViewIfNeeded();
  const drawn = await box.boundingBox();
  if (!drawn) throw new Error('no box of a task');
  const middle = { x: drawn.x + drawn.width / 2, y: drawn.y + drawn.height / 2 };
  const corners = [
    [-20, -20],
    [-20, 20],
    [8, -20],
    [8, 20],
  ];
  const answers = await box.evaluate(
    (input, { x, y, offsets }) =>
      offsets.map(([dx = 0, dy = 0]) => {
        const hit = document.elementFromPoint(x + dx, y + dy);
        return hit?.closest('label') === input.closest('label');
      }),
    { ...middle, offsets: corners },
  );
  expect(answers).toEqual([true, true, true, true]);
  const checked = await box.isChecked();
  await page.touchscreen.tap(middle.x - 18, middle.y + 18);
  await expect(box).toBeChecked({ checked: !checked });
});

test('a wiki: the pages of its tree are a finger high where they are pressed', async ({ page }) => {
  await openApp(page);
  await newWiki(page, 'Handbook');
  await pageTree(page).getByRole('button', { name: 'New page' }).first().click();
  await expect(page.getByTestId('page-title')).toBeFocused();
  await page.keyboard.type('Getting started');
  await page.keyboard.press('Enter');
  const row = pageTree(page).getByRole('treeitem', { name: 'Getting started' });
  await expectFingerSized(row.getByRole('link'), 'the page in the tree');
  expect((await page.getByTestId('page-title').boundingBox())?.height).toBeGreaterThanOrEqual(44);
  await expectPageStill(page);
});
