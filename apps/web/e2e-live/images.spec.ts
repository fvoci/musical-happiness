/**
 * An image in a card's body on the real server: pasted into the editor, it
 * goes up as a file of the board, the stored body names it by its file, and
 * it is drawn again from the server when the card is opened later, in the
 * editor and in the read-only view of somebody who may only look.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

import {
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Person,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

/** A picture of 3 by 2 pixels. */
const PNG =
  'iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAIAAAASFvFNAAAAEElEQVR4nGM4YaMBQQxwFgBKJAcJKumIJAAAAABJRU5ErkJggg==';

interface Node {
  type: string;
  attrs?: Record<string, unknown>;
  content?: Node[];
}

function imagesIn(node: Node | null | undefined): string[] {
  if (!node) return [];
  const own = node.type === 'image' ? [String(node.attrs?.fileId)] : [];
  return [...own, ...(node.content ?? []).flatMap((child) => imagesIn(child))];
}

/** Whether every picture in `place` has arrived and been drawn. */
async function drawn(place: Locator): Promise<number[]> {
  return place
    .locator('img')
    .evaluateAll((images) =>
      images.map((image) =>
        (image as HTMLImageElement).complete ? (image as HTMLImageElement).naturalWidth : -1,
      ),
    );
}

async function openCard(page: Page, path: string, who: Person): Promise<void> {
  await page.goto(path);
  await signInOnPage(page, who);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
}

test('an image pasted into the body is a file of the board, and is drawn from the server again', async ({
  page,
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('painter');
  const onlooker = person('onlooker');
  await register(base, owner);
  await register(base, onlooker);
  const api = await signedIn(base, owner);
  const board = await makeBoard(api, {
    title: 'Pictures',
    cards: [{ title: 'The view from the hill', status: 'To do' }],
    members: [{ who: onlooker, role: 'viewer' }],
  });
  const cardId = board.cards['The view from the hill'] ?? '';
  const path = `/board/${board.boardId}/${board.viewId}/${cardId}`;
  const stored = async (): Promise<Schemas['Card']> =>
    (await (await api.get(`/api/v1/cards/${cardId}`)).json()) as Schemas['Card'];

  const errors = watchErrors(page);
  // Every reading of a file answers with it.
  const files: number[] = [];
  page.on('response', (response) => {
    if (/\/api\/v1\/boards\/[^/]+\/files\//.test(response.url())) files.push(response.status());
  });
  await openCard(page, path, owner);
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  const editor = page.getByTestId('card-body').locator('[contenteditable]').first();
  await editor.click();
  await page.keyboard.type('From the top: ');
  await editor.evaluate((element, png) => {
    const bytes = Uint8Array.from(atob(png), (letter) => letter.charCodeAt(0));
    const data = new DataTransfer();
    data.items.add(new File([bytes], 'hill.png', { type: 'image/png' }));
    element.dispatchEvent(
      new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
    );
  }, PNG);

  // Stored: the body names the picture by a file the board has.
  await expect
    .poll(async () => imagesIn((await stored()).body as unknown as Node), { timeout: 15_000 })
    .toHaveLength(1);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  const [fileId] = imagesIn((await stored()).body as unknown as Node);
  const file = await api.get(`/api/v1/boards/${board.boardId}/files/${fileId ?? ''}`);
  expect(file.status()).toBe(200);
  expect(Buffer.from(await file.body()).equals(Buffer.from(PNG, 'base64'))).toBe(true);
  await expect.poll(() => drawn(editor)).toEqual([3]);

  // Opened again: drawn from the server.
  await page.reload();
  await expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');
  await expect.poll(() => drawn(page.getByTestId('card-body'))).toEqual([3]);
  await expect(page.locator('[data-problem]')).toHaveCount(0);
  expect(files.length).toBeGreaterThan(0);
  expect(files.every((status) => status === 200)).toBe(true);

  // Somebody who may only look sees it in the read-only view.
  const look = await (await browser.newContext({ locale: 'en-US' })).newPage();
  errors.push(...watchErrors(look));
  await openCard(look, path, onlooker);
  await expect(look.getByTestId('card-body').locator('[contenteditable="true"]')).toHaveCount(0);
  await expect.poll(() => drawn(look.getByTestId('card-body'))).toEqual([3]);
  await look.context().close();
  await api.dispose();
  expect(errors).toEqual([]);
});
