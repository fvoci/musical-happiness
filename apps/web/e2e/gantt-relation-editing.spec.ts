/**
 * Four dependency relation kinds (`FS`, `SS`, `FF`, `SF`), lag days, arrow
 * click and keyboard editing, read-only visibility, and hierarchy collapse
 * compatibility in the Gantt view.
 */
import { expect, test } from '@playwright/test';

import { boardPath } from './address.ts';
import {
  arrowEnds,
  arrows,
  bar,
  gantt,
  goTo,
  NOW,
  openRelated,
  setAfterLinks,
  setDates,
  type Backend,
  type Dev,
} from './gantt.support.ts';
import { fitPane, paneOf } from './kanban.support.ts';
import { tabKey } from './keyboard.support.ts';

test.use({ timezoneId: 'Asia/Seoul', locale: 'en-US' });

test('all four relation kinds connect the right bar ends and show kind/lag badges and screen-reader wording', async ({
  page,
}, info) => {
  await openRelated(page, info);
  await setDates(page, {
    'Create a new card': { from: '2026-11-10', to: '2026-11-14' },
    'Drag cards': { from: '2026-11-11', to: '2026-11-15' },
    'Share a board': { from: '2026-11-12', to: '2026-11-16' },
    'Filter and sort cards': { from: '2026-11-12', to: '2026-11-14' },
  });
  await setAfterLinks(page, {
    'Drag cards': [{ card: 'Create a new card', kind: 'ss', lagDays: 2 }],
    'Share a board': [{ card: 'Drag cards', kind: 'ff', lagDays: -1 }],
    'Filter and sort cards': [{ card: 'Share a board', kind: 'sf', lagDays: 1 }],
  });

  await expect(arrows(page)).toHaveCount(3);
  const checkEnds = async (
    from: string,
    to: string,
    fromEnd: boolean,
    toStart: boolean,
  ): Promise<void> => {
    const line = await arrowEnds(page, from, to);
    const first = await bar(page, from).boundingBox();
    const second = await bar(page, to).boundingBox();
    if (!first || !second) throw new Error('missing bars');
    const expectedStart = fromEnd ? first.x + first.width : first.x;
    const expectedEnd = toStart ? second.x : second.x + second.width;
    expect(Math.abs(line.start.x - expectedStart), `${from} → ${to} start`).toBeLessThan(2);
    expect(Math.abs(line.end.x - expectedEnd), `${from} → ${to} end`).toBeLessThan(2);
  };
  await checkEnds('Create a new card', 'Drag cards', false, true); // SS
  await checkEnds('Drag cards', 'Share a board', true, false); // FF
  await checkEnds('Share a board', 'Filter and sort cards', false, false); // SF

  // SS +2d is out of order (starts Nov 11, fewer than 2 days after Nov 10).
  await expect(bar(page, 'Drag cards')).toHaveAccessibleName(
    'number 1, Drag cards, November 11 – November 15. After Create a new card. Out of order: starts fewer than 2 days after the start of Create a new card.',
  );
  // Arrow button has accessible name with kind, lag, and out-of-order status.
  const ssTrigger = gantt(page).locator('[data-arrow-trigger][data-kind="ss"]');
  await expect(ssTrigger).toHaveText('SS +2d');
  await expect(ssTrigger).toHaveAccessibleName(
    'Create a new card to Drag cards, Start to start, +2 days lag. Out of order.',
  );
  await expect(gantt(page).locator('[data-arrow-trigger][data-kind="ff"]')).toHaveText('FF -1d');
  await expect(gantt(page).locator('[data-arrow-trigger][data-kind="sf"]')).toHaveText('SF +1d');
});

test('clicking or keyboard-activating an arrow edits kind and lagDays, preserves hidden links, and deletes', async ({
  page,
}, info) => {
  await openRelated(page, info);
  const ids = await page.evaluate(() => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    return Object.fromEntries(store.cards.map((card) => [card.title, card.id]));
  });
  const fromId = ids['Create a new card'] ?? '';
  const toId = ids['Share a board'] ?? '';

  // Click the SVG hit path of a default FS 0 arrow to open the editor, change FS -> SS and lagDays -> +2.
  const trigger = gantt(page).locator(
    `[data-arrow-trigger][data-from="${fromId}"][data-to="${toId}"]`,
  );
  const hitPath = gantt(page).locator(`[data-arrow-hit][data-from="${fromId}"][data-to="${toId}"]`);
  await hitPath.click({ force: true });
  const editor = page.getByTestId('gantt-relation-editor');
  await expect(editor).toBeVisible();
  await editor.locator('[data-relation-kind="ss"]').click();
  await page.getByTestId('relation-lag-inc').click();
  await page.getByTestId('relation-lag-inc').click();
  await page.getByTestId('relation-save').click();
  await expect(editor).toHaveCount(0);

  await expect(trigger).toHaveText('SS +2d');
  await expect(
    gantt(page).locator(`[data-arrow][data-from="${fromId}"][data-to="${toId}"]`),
  ).toHaveAttribute('data-kind', 'ss');

  // Verify the hidden deleted card link on "Share a board" was preserved untouched.
  const storedAfterEdit = await page.evaluate((cardId) => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const card = store.cards.find((each) => each.id === cardId) as unknown as {
      fields: { properties: Record<string, unknown> };
    };
    return card.fields.properties.after;
  }, toId);
  expect(storedAfterEdit).toHaveLength(2);
  expect(storedAfterEdit).toEqual(
    expect.arrayContaining([{ card: fromId, kind: 'ss', lagDays: 2 }]),
  );

  // Keyboard flow: activate with Enter and remove the relation.
  await trigger.focus();
  await expect(trigger).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(editor).toBeVisible();
  await page.getByTestId('relation-remove').click();
  await expect(editor).toHaveCount(0);
  await expect(
    gantt(page).locator(`[data-arrow][data-from="${fromId}"][data-to="${toId}"]`),
  ).toHaveCount(0);
  // Focus returns to the target card's bar after deletion.
  await expect(bar(page, 'Share a board')).toBeFocused();
  // Hidden link still preserved even after deleting the visible link.
  const storedAfterDelete = await page.evaluate((cardId) => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace.board;
    const card = store.cards.find((each) => each.id === cardId) as unknown as {
      fields: { properties: Record<string, unknown> };
    };
    return card.fields.properties.after;
  }, toId);
  expect(storedAfterDelete).toHaveLength(1);

  // Roving Tab stop: one Tab from a bar reaches the arrow triggers, ArrowDown walks them, Space opens editor.
  await expect(gantt(page).locator('[data-arrow-trigger][tabindex="0"]')).toHaveCount(1);
  await bar(page, 'Share a board').focus();
  await page.keyboard.press(await tabKey(page));
  await expect(gantt(page).locator('[data-arrow-trigger]').first()).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(gantt(page).locator('[data-arrow-trigger]').nth(1)).toBeFocused();
  await page.keyboard.press('Space');
  await expect(editor).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(editor).toHaveCount(0);
});

test('read-only Gantt shows non-default relation badges without edit or delete controls', async ({
  page,
}, info) => {
  await page.clock.setFixedTime(NOW);
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  await fitPane(page, paneOf(info));
  const made = await page.evaluate(async () => {
    const dev = (
      window as unknown as { fvociDev: { backend: Backend; someoneElse?: () => Backend } }
    ).fvociDev;
    const workspaces = await dev.backend.workspaces();
    const boards = await dev.backend.boards(workspaces[0]?.id ?? '');
    const boardId = boards.find((board) => board.title === 'Company Goals & OKRs')?.id ?? '';
    const author = dev.someoneElse?.() ?? dev.backend;
    const board = await author.container(boardId);
    const due = board.cardProperties.find((property) => property.name === 'Due Date');
    if (!due) throw new Error('the board has no Due Date');
    await author.patchContainer(boardId, {
      updatedCardProperties: [{ id: 'after', name: 'After', type: 'relation', options: [] }],
    });
    const first = await author.createCard(boardId, {
      title: 'Alpha',
      properties: { [due.id]: { from: '2026-11-09', to: '2026-11-12' } },
    });
    await author.createCard(boardId, {
      title: 'Beta',
      properties: {
        [due.id]: { from: '2026-11-11', to: '2026-11-14' },
        after: [{ card: first.id, kind: 'ss', lagDays: 3 }],
      },
    });
    const view = await author.createView(boardId, {
      title: 'Read only relations',
      fields: {
        viewType: 'gantt',
        dateDisplayPropertyId: due.id,
        dependencyPropertyId: 'after',
        ganttZoom: 'day',
        sortOptions: [],
        visiblePropertyIds: ['__title'],
        visibleOptionIds: [],
        hiddenOptionIds: [],
        collapsedOptionIds: [],
        filter: { operation: 'and', filters: [] },
        cardOrder: [],
        columnWidths: {},
        columnCalculations: {},
        kanbanCalculations: {},
        defaultTemplateId: '',
      },
    });
    return { boardId, viewId: view.id };
  });
  await goTo(page, boardPath(made.boardId, made.viewId));
  await expect(arrows(page)).toHaveCount(1);
  await expect(gantt(page).locator('[data-arrow-trigger]')).toHaveCount(0);
  await expect(gantt(page).locator('[data-arrow-hit]')).toHaveCount(0);
  const badge = gantt(page).locator('[data-arrow-badge]');
  await expect(badge).toHaveText('SS +3d');
  await expect(badge).toHaveAccessibleName(
    'Alpha to Beta, Start to start, +3 days lag. Out of order.',
  );
  await expect(page.getByTestId('gantt-relation-editor')).toHaveCount(0);
});

test('collapsing a parent row while the relation editor is open closes it cleanly and hides sub-card arrows', async ({
  page,
}, info) => {
  await openRelated(page, info);
  const ids = await page.evaluate(async () => {
    const store = (window as unknown as { fvociDev: Dev }).fvociDev.workspace
      .board as Dev['workspace']['board'] & {
      changeCard(
        cardId: string,
        fn: (card: { id: string; title: string; parentId?: string }) => {
          id: string;
          title: string;
          parentId?: string;
        },
      ): Promise<void>;
    };
    const parent = store.cards.find((card) => card.title === 'Create a new card');
    const child = store.cards.find((card) => card.title === 'Share a board');
    if (!parent || !child) throw new Error('missing cards');
    await store.changeCard(child.id, (card) => ({ ...card, parentId: parent.id }));
    return { parentId: parent.id, childId: child.id };
  });

  const hitPath = gantt(page).locator(
    `[data-arrow-hit][data-from="${ids.parentId}"][data-to="${ids.childId}"]`,
  );
  await expect(hitPath).toHaveCount(1);
  await hitPath.click({ force: true });
  await expect(page.getByTestId('gantt-relation-editor')).toBeVisible();

  // Collapse the parent row directly while the editor is open (works on both popover and mobile sheet).
  const toggle = gantt(page).locator(`[data-hierarchy-toggle="${ids.parentId}"]`);
  await toggle.evaluate((button: HTMLElement) => {
    button.click();
  });
  await expect(page.getByTestId('gantt-relation-editor')).toHaveCount(0);
  await expect(
    gantt(page).locator(`[data-arrow][data-from="${ids.parentId}"][data-to="${ids.childId}"]`),
  ).toHaveCount(0);

  // Expanding restores the child bar and its arrows.
  await toggle.click();
  await expect(
    gantt(page).locator(`[data-arrow][data-from="${ids.parentId}"][data-to="${ids.childId}"]`),
  ).toHaveCount(1);
});
