/** Two independent browser sessions see who is on their board. */
import { expect, test } from '@playwright/test';
import { boardPath, makeBoard, person, register, signedIn, watchErrors } from './support.ts';

test.use({ locale: 'en-US' });

test('board presence shows another person and removes their closed browser', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('present-owner');
  const other = person('present-reader');
  await register(base, owner);
  await register(base, other);
  const api = await signedIn(base, owner);
  const otherApi = await signedIn(base, other);
  const board = await makeBoard(api, {
    title: 'Here together',
    cards: [{ title: 'Discuss the draft' }],
    members: [{ who: other, role: 'viewer' }],
  });
  const firstContext = await browser.newContext({
    baseURL: base,
    storageState: await api.storageState(),
    locale: 'en-US',
  });
  const secondContext = await browser.newContext({
    baseURL: base,
    storageState: await otherApi.storageState(),
    locale: 'en-US',
  });
  const first = await firstContext.newPage();
  const second = await secondContext.newPage();
  const errors = [watchErrors(first), watchErrors(second)];
  try {
    await first.goto(boardPath(board, board.viewId));
    await expect(first.getByTestId('board-presence')).toHaveAccessibleName(
      '0 people viewing this board',
    );
    await second.goto(boardPath(board, board.viewId));
    for (const page of [first, second]) {
      await expect(page.getByTestId('board-presence')).toHaveAccessibleName(
        '1 people viewing this board',
      );
      await expect(page.getByTestId('board-presence').locator('[data-avatar]')).toHaveCount(1);
    }
    await first.getByTestId('board-presence').click();
    await expect(first.getByTestId('presence-list')).toContainText(other.username);
    await expect(first.getByTestId('presence-list')).not.toContainText(owner.username);
    await first.keyboard.press('Escape');
    await secondContext.close();
    await expect(first.getByTestId('board-presence')).toHaveAccessibleName(
      '0 people viewing this board',
    );
    expect(errors.flat()).toEqual([]);
  } finally {
    await firstContext.close();
    await secondContext.close();
    await api.dispose();
    await otherApi.dispose();
  }
});
