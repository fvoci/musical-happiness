/**
 * The box of a task in a body: pressing it changes the task, and nothing else.
 *
 * On a phone the focus of the body is its keyboard: a press on a box that
 * gave the body the focus raised the keyboard, put a caret at the end of the
 * line and showed the selection's bubble, though the reader only meant to
 * tick the task. Here a press with a finger leaves the focus and the
 * selection as they were: none in the body when it had none, the same caret
 * when the reader was writing. The tests see the focus and the selection; the
 * keyboard itself is the phone's and is seen on one.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import { boardPath } from './address.ts';
import { primaryKey } from './keyboard.support.ts';
import { newWiki } from './wiki.support.ts';

test.describe.configure({ mode: 'parallel', timeout: 90_000 });

interface Place {
  boardId: string;
  viewId: string;
  cardId: string;
}

/** What `(window as unknown as { fvociDev: Dev }).fvociDev` offers a test (src/app/backend.ts). Only what is used here. */
interface DevBackend {
  me(): Promise<{ id: string }>;
  workspaces(): Promise<{ id: string }[]>;
  boards(workspaceId: string): Promise<{ id: string; title: string }[]>;
  views(boardId: string): Promise<{ id: string; title: string }[]>;
  cards(boardId: string): Promise<{ id: string; title: string }[]>;
  addMember(boardId: string, userId: string, role: 'admin'): Promise<unknown>;
  setMemberRole(boardId: string, userId: string, role: 'viewer'): Promise<unknown>;
}
interface Dev {
  backend: DevBackend;
  someoneElse(): DevBackend;
}

/** The card of the board of tasks whose body has a description and four tasks. */
async function findCard(page: Page): Promise<Place> {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  return page.evaluate(async () => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const [workspace] = await backend.workspaces();
    const board = (await backend.boards(workspace?.id ?? '')).find((each) =>
      each.title.startsWith('Project Tasks'),
    );
    if (!board) throw new Error('no board of tasks');
    const views = await backend.views(board.id);
    const card = (await backend.cards(board.id)).find(
      (each) => each.title === 'Identify dependencies',
    );
    if (!card) throw new Error('no card');
    const first = [...views].sort((a, b) => a.title.localeCompare(b.title))[0];
    return { boardId: board.id, viewId: first?.id ?? '', cardId: card.id };
  });
}

/** Opens the card and gives its editor once it can be written. */
async function openCard(page: Page, place: Place): Promise<Locator> {
  await page.goto(boardPath(place.boardId, place.viewId, place.cardId));
  return editorOf(page, 'card-body');
}

async function editorOf(page: Page, testId: string): Promise<Locator> {
  const body = page.getByTestId(testId);
  await expect(body).toHaveAttribute('data-editor', 'ready');
  return body.locator('.ProseMirror');
}

const boxes = (editor: Locator): Locator =>
  editor.locator('ul[data-type="taskList"] > li > label input[type="checkbox"]');

/** Whether the body has the focus (not a box in it), whether the selection is in it, the editor's own selection, and how far its scroller is scrolled. */
const where = (
  editor: Locator,
): Promise<{
  focused: boolean;
  selected: boolean;
  caret: [number, number];
  scrolled: number;
}> =>
  editor.evaluate((root) => {
    const selection = getSelection();
    const anchor = selection && selection.rangeCount > 0 ? selection.anchorNode : null;
    let box = root.parentElement;
    while (box && !/auto|scroll/.test(getComputedStyle(box).overflowY)) box = box.parentElement;
    const scroller = box ?? document.scrollingElement ?? document.documentElement;
    const { state } = (
      root as HTMLElement & { editor: { state: { selection: { from: number; to: number } } } }
    ).editor;
    return {
      focused: document.activeElement === root,
      selected: anchor !== null && root.contains(anchor),
      caret: [state.selection.from, state.selection.to],
      scrolled: scroller.scrollTop,
    };
  });

/**
 * A finger, or a pointer, pressed on the middle of a box: where its label answers (on a touch
 * screen the label is a finger's square drawn over the box). Nothing is scrolled first.
 */
async function press(page: Page, box: Locator): Promise<void> {
  const drawn = await box.boundingBox();
  if (!drawn) throw new Error('no box of a task');
  const x = drawn.x + drawn.width / 2;
  const y = drawn.y + drawn.height / 2;
  if (test.info().project.use.hasTouch) await page.touchscreen.tap(x, y);
  else await page.mouse.click(x, y);
}

/** Scrolls the tasks of the body into sight, before what is seen is taken. */
const tasksInSight = (editor: Locator): Promise<void> =>
  boxes(editor).last().scrollIntoViewIfNeeded();

/** The state of each task of the body, as its box shows it. */
const ticks = (editor: Locator): Promise<boolean[]> =>
  boxes(editor).evaluateAll((all) => all.map((box) => (box as HTMLInputElement).checked));

test.describe('with a finger', () => {
  test.beforeEach(({ hasTouch }) => {
    test.skip(!hasTouch, 'a finger needs a touch screen');
  });

  test('a press on a task’s box in a body without the focus ticks it, and the body takes no focus', async ({
    page,
  }) => {
    const editor = await openCard(page, await findCard(page));
    await tasksInSight(editor);
    const before = await where(editor);
    expect(before.focused).toBe(false);
    expect(before.selected).toBe(false);

    await press(page, boxes(editor).nth(1));
    await expect(boxes(editor).nth(1)).toBeChecked();
    expect(await where(editor)).toEqual(before);

    await press(page, boxes(editor).nth(1));
    await expect(boxes(editor).nth(1)).not.toBeChecked();
    expect(await where(editor)).toEqual(before);
  });

  test('a press on a task’s box while writing ticks it, and the caret stays where it was', async ({
    page,
  }) => {
    const editor = await openCard(page, await findCard(page));
    // The caret in the description, away from the tasks.
    await editor.getByText('[Brief description of this task]').tap();
    await expect(editor).toBeFocused();
    await tasksInSight(editor);
    const before = await where(editor);
    expect(before.selected).toBe(true);

    await press(page, boxes(editor).nth(2));
    await expect(boxes(editor).nth(2)).toBeChecked();
    expect(await where(editor)).toEqual(before);
    await expect(editor).toBeFocused();
  });

  test('on a wiki’s page as on a card', async ({ page }) => {
    await page.goto('/');
    await newWiki(page, 'Handbook');
    await page.getByTestId('wiki-first-page').click();
    await expect(page.getByTestId('page-title')).toBeFocused();
    await page.keyboard.type('Chores');
    await page.keyboard.press('Enter');
    const editor = await editorOf(page, 'page-body');
    await editor.click();
    await page.keyboard.type('[ ] ');
    await page.keyboard.insertText('Water the plants');
    await expect(boxes(editor)).toHaveCount(1);
    // Out of the body: the title takes the focus.
    await page.getByTestId('page-title').click();
    const before = await where(editor);
    expect(before.focused).toBe(false);

    await press(page, boxes(editor).first());
    await expect(boxes(editor).first()).toBeChecked();
    expect((await where(editor)).focused).toBe(false);
    expect((await where(editor)).selected).toBe(false);
  });
});

test('Space on a task’s box ticks it, and the box keeps the focus', async ({ page }) => {
  const editor = await openCard(page, await findCard(page));
  const box = boxes(editor).first();
  await box.focus();
  await page.keyboard.press('Space');
  await expect(box).toBeChecked();
  await expect(box).toBeFocused();
  await page.keyboard.press('Space');
  await expect(box).not.toBeChecked();
});

test('a click on a task’s box ticks it without moving the caret of the body being written', async ({
  page,
}) => {
  const editor = await openCard(page, await findCard(page));
  await editor.getByText('[Brief description of this task]').click();
  await expect(editor).toBeFocused();
  await tasksInSight(editor);
  const before = await where(editor);

  await press(page, boxes(editor).first());
  await expect(boxes(editor).first()).toBeChecked();
  expect(await where(editor)).toEqual(before);
  await expect(editor).toBeFocused();
});

test('a reader who may only read cannot tick a task', async ({ page }) => {
  const place = await findCard(page);
  const editor = await openCard(page, place);
  await page.evaluate(async ({ boardId }) => {
    const dev = (window as unknown as { fvociDev: Dev }).fvociDev;
    // A board keeps an admin: somebody else is one before the reader is made a viewer.
    await dev.backend.addMember(boardId, (await dev.someoneElse().me()).id, 'admin');
    await dev.someoneElse().setMemberRole(boardId, (await dev.backend.me()).id, 'viewer');
  }, place);
  await expect(editor).toHaveAttribute('contenteditable', 'false');
  const before = await ticks(editor);

  const box = boxes(editor).first();
  await tasksInSight(editor);
  await press(page, box);
  expect(await ticks(editor)).toEqual(before);
  expect((await where(editor)).focused).toBe(false);
  await box.focus();
  await page.keyboard.press('Space');
  expect(await ticks(editor)).toEqual(before);
});

test('ticked in one tab, the task is ticked in the other once, and one undo takes it back in both', async ({
  page,
}) => {
  const place = await findCard(page);
  const mine = await openCard(page, place);
  const other = await page.context().newPage();
  const theirs = await openCard(other, place);
  const untouched = await ticks(mine);
  expect(await ticks(theirs)).toEqual(untouched);

  // The other is writing in the description meanwhile: their caret is in the room.
  await theirs.getByText('[Brief description of this task]').click();
  await expect(mine.locator('.doc-caret')).toHaveCount(1);

  const changes = await mine.evaluate((root) => {
    const { editor } = root as HTMLElement & {
      editor: {
        on(event: string, run: (args: { transaction: { docChanged: boolean } }) => void): void;
      };
    };
    const seen = { count: 0 };
    editor.on('transaction', ({ transaction }) => {
      if (transaction.docChanged) seen.count += 1;
    });
    (window as unknown as { seen: typeof seen }).seen = seen;
    return seen.count;
  });
  expect(changes).toBe(0);

  await tasksInSight(mine);
  await press(page, boxes(mine).nth(3));
  const ticked = untouched.map((each, index) => (index === 3 ? !each : each));
  await expect.poll(() => ticks(mine)).toEqual(ticked);
  await expect.poll(() => ticks(theirs)).toEqual(ticked);
  expect(
    await page.evaluate(() => (window as unknown as { seen: { count: number } }).seen.count),
  ).toBe(1);
  expect((await where(mine)).focused).toBe(false);

  // Undo is the body's: it is pressed in it.
  await mine.getByText('[Brief description of this task]').click();
  await page.keyboard.press(await primaryKey(page, 'z'));
  await expect.poll(() => ticks(mine)).toEqual(untouched);
  await expect.poll(() => ticks(theirs)).toEqual(untouched);
});
