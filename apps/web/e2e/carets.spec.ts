/**
 * The carets of a body edited together, end to end on the in-memory backend:
 * two tabs of one browser have the same card open (the rooms in memory tell
 * each other's carets, as the server's do), and each shows where the other
 * is, with the reader's name for a while after they moved.
 *
 * A syllable being composed with an input method is typed through the
 * browser's own input method interface (`Input.imeSetComposition`), which
 * drives the same composition events a keyboard's does.
 *
 * `FVOCI_SCREENS=1` also takes the pictures of the carets, into
 * `target/screens/caret/` (`e2e/screens.ts`).
 */
import path from 'node:path';

import { expect, test, type Locator, type Page } from '@playwright/test';

import { boardPath } from './address.ts';
import { screensDir } from './screens.ts';

test.describe.configure({ mode: 'parallel', timeout: 90_000 });

interface Place {
  boardId: string;
  viewId: string;
  cardId: string;
}

interface DevBackend {
  workspaces(): Promise<{ id: string }[]>;
  boards(workspaceId: string): Promise<{ id: string; title: string }[]>;
  views(boardId: string): Promise<{ id: string; title: string }[]>;
  cards(boardId: string): Promise<{ id: string; title: string }[]>;
}
interface Dev {
  backend: DevBackend;
}

/** A card of the board of tasks, by its title. */
async function find(page: Page, card: string): Promise<Place> {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  return page.evaluate(async (cardTitle) => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const [workspace] = await backend.workspaces();
    const boards = await backend.boards(workspace?.id ?? '');
    const found = boards.find((each) => each.title.startsWith('Project Tasks'));
    if (!found) throw new Error('no board of tasks');
    const views = await backend.views(found.id);
    const wanted = (await backend.cards(found.id)).find((each) => each.title === cardTitle);
    if (!wanted) throw new Error(`no card ${cardTitle}`);
    const first = [...views].sort((a, b) => a.title.localeCompare(b.title))[0];
    return { boardId: found.id, viewId: first?.id ?? '', cardId: wanted.id };
  }, card);
}

async function bodyEditor(page: Page): Promise<Locator> {
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  return page.getByTestId('card-body').locator('[contenteditable]').first();
}

/**
 * The same card open in two tabs: the editors of its body, and the second
 * tab. With `body`, the first tab's reader writes those lines in place of
 * what the body held (each tab has a world of its own, made alike: what one
 * writes reaches the other through their rooms), and leaves the body again.
 */
async function twoTabs(
  page: Page,
  body?: string[],
): Promise<{ mine: Locator; theirs: Locator; other: Page }> {
  const place = await find(page, 'Identify dependencies');
  await page.goto(boardPath(place.boardId, place.viewId, place.cardId));
  const mine = await bodyEditor(page);
  const other = await page.context().newPage();
  await other.goto(boardPath(place.boardId, place.viewId, place.cardId));
  const theirs = await bodyEditor(other);
  if (body) {
    await mine.click();
    await page.keyboard.press('Control+A');
    await page.keyboard.press('Delete');
    for (const [index, line] of body.entries()) {
      if (index > 0) await page.keyboard.press('Enter');
      await page.keyboard.insertText(line);
    }
    await expect(mine.locator('p')).toHaveText(body);
    await page.getByTestId('card-title').click();
  }
  // Each has what the other's room holds, and no caret of the other is left.
  await expect(theirs).toHaveText((await mine.textContent()) ?? '');
  await expect(theirs.locator('.doc-caret')).toHaveCount(0);
  return { mine, theirs, other };
}

const caret = (editor: Locator): Locator => editor.locator('.doc-caret');
const nameOf = (editor: Locator): Locator => editor.locator('.doc-caret-name');

test("each tab shows where the other is, with the reader's name for a while after they moved", async ({
  page,
}) => {
  const { mine, theirs, other } = await twoTabs(page, ['The first line.', 'The second line.']);
  await expect(caret(mine)).toHaveCount(0);

  // The other tab's reader goes into the body: a caret, and their name.
  await theirs.locator('p').nth(1).click();
  await expect(caret(mine)).toHaveCount(1);
  await expect(caret(mine)).toHaveAttribute('data-named', '');
  await expect(nameOf(mine)).toHaveAttribute('data-name', /\S/);
  await expect(nameOf(mine)).toHaveCSS('opacity', '1');
  // It is in the line they are in, and is no part of what a screen reader is told.
  await expect(mine.locator('p').nth(1).locator('.doc-caret')).toHaveCount(1);
  await expect(caret(mine)).toHaveAttribute('aria-hidden', 'true');
  // Nobody is shown their own caret.
  await expect(caret(theirs)).toHaveCount(0);

  // The name goes after a few seconds; the line stays.
  await expect(caret(mine)).not.toHaveAttribute('data-named', '', { timeout: 6000 });
  await expect(nameOf(mine)).toHaveCSS('opacity', '0');
  await expect(caret(mine)).toHaveCount(1);
  // They write: the caret follows, and the name is back.
  await other.keyboard.press('End');
  await other.keyboard.type(' More.');
  await expect(mine.locator('p').nth(1)).toHaveText('The second line. More.');
  await expect(caret(mine)).toHaveAttribute('data-named', '');
  // A selection of theirs is tinted.
  await other.keyboard.press('Shift+Home');
  await expect(mine.locator('.doc-caret-selection').first()).toBeVisible();

  // Both ways: the first tab's caret shows in the second.
  await mine.locator('p').first().click();
  await expect(caret(theirs)).toHaveCount(1);
  // Leaving the body takes the caret away.
  await page.getByTestId('card-title').click();
  await expect(caret(theirs)).toHaveCount(0);
  // (A tab that goes away says nothing to the rooms in memory: its caret is let go thirty
  // seconds after its last word. The server tells at once: its tests, and those against it.)
  await other.close();
});

test('a name is not cut off at the edges: under the first line, and leftwards at the right edge', async ({
  page,
}) => {
  // A word too long for a row is cut at the right edge: the end of its first row is at the edge.
  const { mine, theirs, other } = await twoTabs(page, ['W'.repeat(400), 'Below.']);
  const inside = async (): Promise<void> => {
    await expect(caret(mine)).toHaveAttribute('data-named', '');
    // Placed a frame after it is drawn.
    await expect
      .poll(async () => {
        const [box, name] = await Promise.all([mine.boundingBox(), nameOf(mine).boundingBox()]);
        if (!box || !name) return 'not drawn';
        const fits =
          name.x >= box.x - 1 &&
          name.x + name.width <= box.x + box.width + 1 &&
          name.y >= box.y - 1;
        return fits ? 'inside' : `outside: ${JSON.stringify({ box, name })}`;
      })
      .toBe('inside');
  };

  // At the very start of the body: nothing above to draw on.
  await theirs.locator('p').first().click();
  await other.keyboard.press('Control+Home');
  await expect(caret(mine)).toHaveAttribute('data-below', '');
  await inside();
  // One letter before the end of the first row of the word: at the right edge. (The very end
  // of a row is the start of the next, where a caret is drawn.)
  await other.keyboard.press('End');
  await other.keyboard.press('ArrowLeft');
  await inside();
  await expect(caret(mine)).toHaveAttribute('data-leftwards', '');
  // In the line below there is room above.
  await other.keyboard.press('Control+End');
  await expect(caret(mine)).not.toHaveAttribute('data-below', '');
  await inside();
  await other.close();
});

test('a caret never blinks, and its name does not fade for a reader who asked for less motion', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const { mine, theirs, other } = await twoTabs(page);
  await theirs.locator('p').first().click();
  await expect(caret(mine)).toHaveCount(1);
  await expect(caret(mine)).toHaveCSS('animation-name', 'none');
  await expect(nameOf(mine)).toHaveCSS('animation-name', 'none');
  // No time to speak of: the app's own rule for less motion leaves a hundredth of a millisecond.
  const fades = (): Promise<number> =>
    nameOf(mine).evaluate((name) => Number.parseFloat(getComputedStyle(name).transitionDuration));
  expect(await fades()).toBeLessThan(0.001);
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await expect(caret(mine)).toHaveCSS('animation-name', 'none');
  expect(await fades()).toBeGreaterThan(0.1);
  await other.close();
});

test('a syllable being composed is kept while the other moves and writes in the same line', async ({
  page,
}) => {
  const { mine, theirs, other } = await twoTabs(page, ['가나다 라마바', 'Below.']);
  const ime = await page.context().newCDPSession(page);
  const compose = (text: string): Promise<unknown> =>
    ime.send('Input.imeSetComposition', {
      text,
      selectionStart: text.length,
      selectionEnd: text.length,
    });

  // The reader composes at the end of the first line: ㅎ, 하, 한.
  await mine.locator('p').first().click();
  await page.keyboard.press('End');
  await compose('ㅎ');
  await compose('하');
  await expect(mine.locator('p').first()).toHaveText('가나다 라마바하');

  // Meanwhile the other comes into the same line, moves, and writes at its start.
  await theirs.locator('p').first().click();
  await other.keyboard.press('Home');
  await expect(caret(mine)).toHaveCount(1);
  await other.keyboard.press('ArrowRight');
  await other.keyboard.type('앞에 ');
  await expect(theirs.locator('p').first()).toContainText('앞에 ');
  // And writes in another line.
  await other.keyboard.press('Control+End');
  await other.keyboard.type(' 아래.');
  await expect(theirs.locator('p').nth(1)).toHaveText('Below. 아래.');

  // The syllable is still being composed, where it was: it goes on, and is committed once.
  expect(await page.evaluate(() => document.querySelector('.ProseMirror')?.textContent)).toContain(
    '하',
  );
  await compose('한');
  await ime.send('Input.insertText', { text: '한' });
  await page.keyboard.type('글');
  await expect(mine.locator('p').first()).toHaveText('가앞에 나다 라마바한글');
  await expect(theirs.locator('p').first()).toHaveText('가앞에 나다 라마바한글');
  await expect(mine.locator('p').nth(1)).toHaveText('Below. 아래.');
  await other.close();
});

test('the pictures of the carets', async ({ page }, testInfo) => {
  test.skip(!process.env.FVOCI_SCREENS, 'set FVOCI_SCREENS=1 to take the pictures');
  test.skip(testInfo.project.name !== 'desktop', 'the sizes are set here');
  const long = 'A line long enough to reach the right edge of the body and to wrap beyond it. ';
  for (const [size, width, height] of [
    ['wide', 1280, 800],
    ['390', 390, 844],
  ] as const) {
    for (const scheme of ['light', 'dark'] as const) {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme: scheme });
      const { mine, theirs, other } = await twoTabs(page, [long.repeat(2), 'The second line.']);
      await other.setViewportSize({ width, height });
      const shot = async (name: string): Promise<void> => {
        await expect(caret(mine)).toHaveAttribute('data-named', '');
        await page.screenshot({
          path: path.join(screensDir('caret'), `${name}-${size}-${scheme}.png`),
        });
      };
      await theirs.locator('p').nth(1).click();
      await other.keyboard.press('Home');
      await other.keyboard.press('Shift+End');
      await shot('selection');
      await other.keyboard.press('Control+Home');
      await other.keyboard.press('End');
      await other.keyboard.press('ArrowLeft');
      await expect(caret(mine)).toHaveAttribute('data-leftwards', '');
      await shot('right-edge');
      await other.keyboard.press('Control+Home');
      await expect(caret(mine)).toHaveAttribute('data-below', '');
      await shot('first-line');
      await other.close();
    }
  }
});
