/**
 * Undo and redo on the real server where one of their writes is refused
 * (the page's request answered 503 by the test): a change made while it was
 * written is undone to the value the server holds, not to the one the
 * refused write showed. Found by a review of 042ff43.
 */
import { expect, test } from '@playwright/test';

import { boardPath, makeBoard, person, register, signedIn, signInOnPage } from './support.ts';

test.use({ locale: 'en-US' });

for (const action of ['undo', 'redo'] as const) {
  test(`a refused ${action} does not corrupt the inverse of an edit made meanwhile`, async ({
    page,
    baseURL,
  }) => {
    const base = baseURL ?? '';
    const owner = person(`undo-${action}`);
    await register(base, owner);
    const api = await signedIn(base, owner);
    const board = await makeBoard(api, { title: 'Alpha' });
    const path = `/api/v1/w/${board.workspaceId}/boards/${board.boardId}`;
    const savedTitle = async (): Promise<string> => {
      const response = await api.get(path);
      expect(response.ok()).toBe(true);
      return ((await response.json()) as { title: string }).title;
    };
    let release = (): void => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });

    try {
      await page.goto('/');
      await signInOnPage(page, owner);
      await page.goto(boardPath(board, board.viewId));
      const title = page.getByTestId('board-title');
      const undo = page.getByTestId('undo');
      await title.fill('One');
      await title.press('Enter');
      await expect.poll(savedTitle).toBe('One');
      if (action === 'redo') {
        await undo.click();
        await expect(undo).not.toHaveAttribute('aria-busy', 'true');
        await expect.poll(savedTitle).toBe('Alpha');
      }

      let entered = (): void => undefined;
      const started = new Promise<void>((resolve) => {
        entered = resolve;
      });
      let refused = false;
      const optimistic = action === 'undo' ? 'Alpha' : 'One';
      await page.route(`**${path}`, async (route) => {
        const request = route.request();
        if (
          !refused &&
          request.method() === 'PATCH' &&
          (request.postDataJSON() as { title?: string }).title === optimistic
        ) {
          refused = true;
          entered();
          await gate;
          await route.fulfill({
            status: 503,
            contentType: 'application/problem+json',
            body: JSON.stringify({
              type: 'about:blank',
              title: 'Service unavailable',
              status: 503,
            }),
          });
        } else {
          await route.continue();
        }
      });

      await page.getByTestId(action).click();
      await started;
      await expect(undo).toHaveAttribute('aria-busy', 'true');
      await expect(title).toHaveValue(optimistic);
      await title.fill('Two');
      await title.press('Enter');
      await expect.poll(savedTitle).toBe('Two');
      release();
      await expect(undo).not.toHaveAttribute('aria-busy', 'true');
      // The refused write does not take what was changed over it off the screen.
      await expect(title).toHaveValue('Two');

      const written = page.waitForResponse(
        (response) => response.url().endsWith(path) && response.request().method() === 'PATCH',
      );
      await undo.click();
      expect((await written).ok()).toBe(true);
      expect(await savedTitle()).toBe(action === 'undo' ? 'One' : 'Alpha');
    } finally {
      release();
      await api.dispose();
    }
  });
}
