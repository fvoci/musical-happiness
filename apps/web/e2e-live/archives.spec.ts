/**
 * Archives on the real server: a board exported from its menu, every board
 * from the settings, both read with their progress and saved as the
 * server names them; an archive imported in parts makes its boards again;
 * a file that is no archive is refused and told.
 */
import { readFile } from 'node:fs/promises';

import { expect, test, type APIRequestContext } from '@playwright/test';

import { boardLink, boardMenu, chooseFromMenu, sidebar } from '../e2e/shell.support.ts';

import {
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

async function boardsTitled(api: APIRequestContext, teamId: string, title: string) {
  const boards = (await (await api.get(`/api/v1/w/${teamId}/boards`)).json()) as Schemas['Board'][];
  return boards.filter((board) => board.title === title);
}

test('a board is exported and imported again, and a file that is no archive is refused', async ({
  page,
  baseURL,
}, testInfo) => {
  const base = baseURL ?? '';
  const owner = person('archivist');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const title = `Archive ${Date.now().toString(36)}`;
  const made = await makeBoard(api, {
    title,
    cards: [
      { title: 'First', status: 'To do' },
      { title: 'Second', status: 'Done' },
    ],
  });

  const errors = watchErrors(page);
  await page.goto(`/board/${made.boardId}/${made.viewId}`);
  await signInOnPage(page, owner);

  // The board, from its menu: a ZIP saved under the server's name.
  const saving = page.waitForEvent('download');
  await chooseFromMenu(page, boardMenu(page, title), 'export');
  const download = await saving;
  expect(download.suggestedFilename()).toMatch(/^archive-\d{4}-\d{2}-\d{2}\.boardarchive$/);
  const archive = testInfo.outputPath('board.boardarchive');
  await download.saveAs(archive);
  expect((await readFile(archive)).subarray(0, 2).toString()).toBe('PK');
  await expect(
    page.getByTestId('transfers').locator('[data-transfer="export"][data-finished]'),
  ).toContainText(`${title} is saved.`);

  // Every board, from the settings.
  await sidebar(page).getByTestId('open-settings').click();
  const settings = page.getByTestId('settings-dialog');
  const savingAll = page.waitForEvent('download');
  await settings.getByTestId('export-team').click();
  const all = await savingAll;
  const allPath = testInfo.outputPath('team.boardarchive');
  await all.saveAs(allPath);
  expect((await readFile(allPath)).subarray(0, 2).toString()).toBe('PK');

  // Imported: the board is made again, with its cards.
  await settings.getByTestId('import-archive').setInputFiles(archive);
  await expect(
    page.getByTestId('transfers').locator('[data-transfer="import"][data-finished]'),
  ).toContainText('board.boardarchive is imported.');
  await expect.poll(async () => (await boardsTitled(api, made.teamId, title)).length).toBe(2);
  const copy = (await boardsTitled(api, made.teamId, title)).find(
    (board) => board.id !== made.boardId,
  );
  const cards = (await (
    await api.get(`/api/v1/boards/${copy?.id ?? ''}/cards`)
  ).json()) as Schemas['Card'][];
  expect(cards.map((card) => card.title).sort()).toEqual(['First', 'Second']);
  await page.keyboard.press('Escape');
  await expect(boardLink(page, title)).toHaveCount(2);

  // No archive: refused, told, nothing made.
  await sidebar(page).getByTestId('open-settings').click();
  await settings.getByTestId('import-archive').setInputFiles({
    name: 'notes.boardarchive',
    mimeType: 'application/zip',
    buffer: Buffer.from('these are not boards'),
  });
  await expect(page.locator('[data-problem="import"]')).toHaveText(
    /That file is no archive of boards this app can read\./,
  );
  expect(await boardsTitled(api, made.teamId, title)).toHaveLength(2);

  await api.dispose();
  expect(errors).toEqual([]);
});
