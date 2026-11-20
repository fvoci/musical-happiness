/**
 * What a save of a Hangul document asks before it changes the file in a way
 * the writer does not see in the editor: a distribution document (배포용
 * 문서) saved as an open one, and streams of an HWP that rhwp does not keep.
 * One question, asked before anything is uploaded, in the dialog that asks
 * about rhwp's own losses; not answered, nothing is saved.
 *
 * With `FVOCI_SCREENS=1` the tests take pictures of the questions into
 * `target/viewer-h3/screens/`.
 */
import { expect, test, type Page } from '@playwright/test';

import { distributionHwp, withViewTextLeft } from '../tests/support/hwp-files.ts';

import { openCard, picture, viewer } from './file-viewer.support.ts';
import { edit, editor, HWP_TYPE, said, type, TYPED } from './hwp-editor.support.ts';

const SCREENS = 'viewer-h3/screens';

const losses = (page: Page) => page.getByTestId('file-editor-losses');

/** Records the requests that would change something: uploads and versions. */
function writes(page: Page): string[] {
  const made: string[] = [];
  page.on('request', (request) => {
    if (request.method() !== 'GET') made.push(new URL(request.url()).pathname);
  });
  return made;
}

test('a distribution document is said to be one before it is saved, once, and asked about', async ({
  page,
}) => {
  await openCard(page);
  await edit(page, '배포용.hwp', distributionHwp(), HWP_TYPE);
  await type(page, TYPED);
  const made = writes(page);

  await page.getByTestId('file-editor-save').click();
  const asked = losses(page);
  await expect(asked).toBeVisible();
  // One question, named by what it asks, the protection said first; nothing more to list.
  await expect(page.getByRole('alertdialog')).toHaveCount(1);
  await expect(
    page.getByRole('alertdialog', { name: 'This is a distribution document' }),
  ).toHaveAccessibleDescription(
    /Hangul keeps a distribution document from being copied or printed/,
  );
  await expect(asked.locator('li')).toHaveCount(0);
  // The keyboard is in the question, which is read by its name and words; no choice is made
  // for the writer, and the first one Tab reaches is not saving.
  await expect(page.getByRole('alertdialog')).toBeFocused();
  await picture(page, SCREENS, 'question-unprotected');
  await page.keyboard.press('Tab');
  await expect(asked.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(asked).toHaveCount(0);
  await expect(said(page)).toHaveText('There are unsaved changes');
  await expect(editor(page)).toHaveAttribute('data-dirty', 'true');
  expect(made).toEqual([]);

  // Asked again, and saved as the file's new version.
  await page.keyboard.press('Control+s');
  await expect(asked).toBeVisible();
  await page.getByTestId('file-editor-losses-version').click();
  await expect(said(page)).toHaveText('Saved as version 2.');

  // The file is no distribution document any more: the next save asks nothing.
  await type(page, '!');
  await page.getByTestId('file-editor-save').click();
  await expect(said(page)).toHaveText('Saved as version 3.');
  await expect(asked).toHaveCount(0);
  await page.getByTestId('file-editor-close').click();
  await expect(viewer(page)).toHaveAttribute('data-status', 'shown');
});

test('streams of an HWP that rhwp does not keep are named, and asked about first', async ({
  page,
}) => {
  await openCard(page);
  // A protected body left in a document that is no distribution document: rhwp does not keep it.
  await edit(page, '남은 본문.hwp', withViewTextLeft(), HWP_TYPE);
  await type(page, TYPED);
  const made = writes(page);
  await page.getByTestId('file-editor-save').click();
  const asked = losses(page);
  await expect(page.getByRole('alertdialog')).toHaveCount(1);
  await expect(
    page.getByRole('alertdialog', { name: 'Saving loses part of the document' }),
  ).toBeVisible();
  await expect(asked).not.toContainText('distribution');
  await expect(asked.locator('li')).toHaveText([
    /^A part of the file is left out: the editor keeps only what it reads\s*ViewText\/Section0$/,
  ]);
  await picture(page, SCREENS, 'question-streams');
  await asked.getByRole('button', { name: 'Cancel' }).click();
  await expect(asked).toHaveCount(0);
  await expect(said(page)).toHaveText('There are unsaved changes');
  expect(made).toEqual([]);
});
