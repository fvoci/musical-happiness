/**
 * What the tests of the file viewer opened from a card share: the card
 * they are tried on, a file attached to it, the viewer and its frame, and
 * pictures of them (`FVOCI_SCREENS=1`, into `target/viewer-p2/`, light and
 * dark, the phone at 390 pixels).
 */
import path from 'node:path';

import { expect, test, type FrameLocator, type Locator, type Page } from '@playwright/test';

import { boardPath } from './address.ts';

const SCREENS = process.env.FVOCI_SCREENS === '1';
const SCREENS_DIR = path.resolve(import.meta.dirname, '../../../target/viewer-p2');

interface Dev {
  backend: {
    workspaces(): Promise<{ id: string }[]>;
    boards(workspaceId: string): Promise<{ id: string; title: string }[]>;
    views(boardId: string): Promise<{ id: string; title: string }[]>;
    cards(boardId: string): Promise<{ id: string; title: string }[]>;
  };
}

/** Opens the card everything is tried on, by its address. */
export async function openCard(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  const place = await page.evaluate(async () => {
    const { backend } = (window as unknown as { fvociDev: Dev }).fvociDev;
    const [workspace] = await backend.workspaces();
    const boards = await backend.boards(workspace?.id ?? '');
    const board = boards.find((each) => each.title.startsWith('Project Tasks'));
    if (!board) throw new Error('no board of tasks');
    const views = await backend.views(board.id);
    const card = (await backend.cards(board.id)).find(
      (each) => each.title === 'Identify dependencies',
    );
    if (!card) throw new Error('no card');
    const first = [...views].sort((a, b) => a.title.localeCompare(b.title))[0];
    return { boardId: board.id, viewId: first?.id ?? '', cardId: card.id };
  });
  await page.goto(boardPath(place.boardId, place.viewId, place.cardId));
  await expect(page.getByTestId('card-attachments')).toBeVisible();
}

/** Attaches a file to the card; gives its row. */
export async function attach(
  page: Page,
  name: string,
  buffer: Buffer,
  mimeType: string,
): Promise<Locator> {
  await page.getByTestId('attach-input').setInputFiles({ name, mimeType, buffer });
  const row = page.getByTestId('card-attachments').locator('[data-attachment]', { hasText: name });
  await expect(row).toBeVisible();
  return row;
}

export const viewer = (page: Page): Locator => page.getByTestId('file-viewer');
export const frameOf = (page: Page): FrameLocator =>
  page.frameLocator('[data-testid="file-viewer-frame"]');

/** Takes the pictures of the page as it is, light and dark, when they are asked for. */
export async function picture(page: Page, name: string): Promise<void> {
  if (!SCREENS) return;
  const project = test.info().project.name;
  if (project === 'mobile') await page.setViewportSize({ width: 390, height: 844 });
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await page.waitForFunction(
      (dark) =>
        document.documentElement.classList.contains('dark') === dark &&
        document.getAnimations().every((animation) => animation.playState !== 'running'),
      theme === 'dark',
    );
    await page.screenshot({ path: path.join(SCREENS_DIR, `${name}-${project}-${theme}.png`) });
  }
  await page.emulateMedia({ colorScheme: null });
}
