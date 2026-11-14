/**
 * Two people in one body on the real server: each sees where the other is,
 * under the name the server knows them by; a viewer sees the carets and
 * shows none; and a connection cut for five seconds leaves the body
 * read-only, both go on from what they had, and both hold the same once it
 * is back, with their carets.
 *
 * `FVOCI_SCREENS=1` also takes the pictures of the carets, into
 * `target/screens/caret/` (`e2e/screens.ts`).
 */
import path from 'node:path';

import {
  expect,
  test,
  type APIRequestContext,
  type Browser,
  type Locator,
  type Page,
  type WebSocketRoute,
} from '@playwright/test';

import { screensDir } from '../e2e/screens.ts';

import {
  boardPath,
  makeBoard,
  person,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

const editor = (page: Page): Locator =>
  page.getByTestId('card-body').locator('[contenteditable]').first();
const carets = (page: Page): Locator => editor(page).locator('.doc-caret');

async function pageOf(
  browser: Browser,
  address: string,
  who: Parameters<typeof signInOnPage>[1],
): Promise<Page> {
  const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
  await page.goto(address);
  await signInOnPage(page, who);
  await expect(page.getByTestId('card-dialog')).toBeVisible();
  return page;
}

const ready = (page: Page): Promise<void> =>
  expect(page.getByTestId('card-body')).toHaveAttribute('data-editor', 'ready');

async function userId(api: APIRequestContext): Promise<string> {
  return ((await (await api.get('/api/v1/users/me')).json()) as Schemas['User']).id;
}

/** The lines of the body as an editor shows them. */
const lines = (page: Page): Promise<string[]> => editor(page).locator('p').allTextContents();

async function picture(page: Page, name: string): Promise<void> {
  if (process.env.FVOCI_SCREENS !== '1') return;
  const wide = page.viewportSize() ?? { width: 1280, height: 720 };
  await page.screenshot({ path: path.join(screensDir('caret'), `live-${name}-wide.png`) });
  await page.setViewportSize({ width: 390, height: 844 });
  // The card has taken its frame for this width, and the names their places in it.
  await expect
    .poll(async () => {
      const box = await editor(page).boundingBox();
      const tag = await carets(page).first().locator('.doc-caret-name').boundingBox();
      return box && tag && box.x + box.width <= 391 && tag.x + tag.width <= 391;
    })
    .toBe(true);
  await page.screenshot({ path: path.join(screensDir('caret'), `live-${name}-390.png`) });
  await page.setViewportSize(wide);
}

test('two people see where the other is, by the name the server knows them by; a viewer sees them and shows none', async ({
  browser,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const [ada, bora, vera] = [person('ada'), person('bora'), person('vera')];
  for (const who of [ada, bora, vera]) await register(base, who);
  const api = await signedIn(base, ada);
  const board = await makeBoard(api, {
    title: 'Carets',
    cards: [{ title: 'Write together', status: 'To do' }],
    members: [
      { who: bora, role: 'editor' },
      { who: vera, role: 'viewer' },
    ],
  });
  const address = boardPath(board, board.viewId, board.cards['Write together'] ?? '');
  const [adaId, boraId] = [await userId(api), await userId(await signedIn(base, bora))];
  const first = await pageOf(browser, address, ada);
  const second = await pageOf(browser, address, bora);
  const third = await pageOf(browser, address, vera);
  const watched = [first, second, third].map((page) => watchErrors(page));
  await ready(first);
  await ready(second);
  await expect(third.getByTestId('card-body')).toHaveAttribute('data-editor', 'reading');

  // Ada writes two lines; Bora goes into the second.
  await editor(first).click();
  await first.keyboard.type('The plan for the week.');
  await first.keyboard.press('Enter');
  await first.keyboard.type('Who books the room?');
  await expect(editor(second).locator('p')).toHaveText([
    'The plan for the week.',
    'Who books the room?',
  ]);
  await editor(second).locator('p').nth(1).click();
  await second.keyboard.press('End');

  // Each sees the other, where they are, under the server's word of who they are.
  await expect(carets(first)).toHaveCount(1);
  await expect(carets(first)).toHaveAttribute('data-user', boraId);
  await expect(carets(first).locator('.doc-caret-name')).toHaveAttribute(
    'data-name',
    new RegExp(bora.username),
  );
  await expect(editor(first).locator('p').nth(1).locator('.doc-caret')).toHaveCount(1);
  await expect(carets(second)).toHaveCount(1);
  await expect(carets(second)).toHaveAttribute('data-user', adaId);
  await expect(carets(second).locator('.doc-caret-name')).toHaveAttribute(
    'data-name',
    new RegExp(ada.username),
  );
  // The carets are no part of the text: both hold the same lines.
  expect(await lines(first)).toEqual(await lines(second));

  // The viewer sees both, and shows none though they press on the body.
  await expect(carets(third)).toHaveCount(2);
  await editor(third).click({ force: true });
  await second.keyboard.type(' Bora does.');
  await expect(editor(first).locator('p').nth(1)).toHaveText('Who books the room? Bora does.');
  await expect(carets(first)).toHaveCount(1);
  await expect(carets(second)).toHaveCount(1);
  await expect(carets(first)).toHaveAttribute('data-named', '');
  await picture(first, 'two-people');
  // A selection of Bora's is tinted for the others.
  await second.keyboard.press('Shift+Home');
  await expect(editor(first).locator('.doc-caret-selection').first()).toBeVisible();
  await expect(carets(first)).toHaveAttribute('data-named', '');
  await picture(first, 'selection');

  // Bora leaves the card: her caret goes at once, for the writer and the viewer.
  await second.context().close();
  await expect(carets(first)).toHaveCount(0);
  await expect(carets(third)).toHaveCount(1);
  for (const page of [first, third]) await expect(page.locator('[data-problem]')).toHaveCount(0);
  await first.context().close();
  await third.context().close();
  await api.dispose();
  expect(watched.flat()).toEqual([]);
});

test('a connection cut for five seconds: the body is read-only meanwhile, and both hold the same once it is back, also after a reload', async ({
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const base = baseURL ?? '';
  const [ada, bora] = [person('ada'), person('bora')];
  for (const who of [ada, bora]) await register(base, who);
  const api = await signedIn(base, ada);
  const board = await makeBoard(api, {
    title: 'Tunnel',
    cards: [{ title: 'Travel notes', status: 'To do' }],
    members: [{ who: bora, role: 'editor' }],
  });
  const cardId = board.cards['Travel notes'] ?? '';
  const address = boardPath(board, board.viewId, cardId);
  const first = await pageOf(browser, address, ada);
  const second = await (await browser.newContext({ locale: 'en-US' })).newPage();
  const watched = [first, second].map((page) => watchErrors(page));
  // Bora's sockets go through here: `held`, nothing passes either way though the
  // connection stands (a line gone quiet); `away`, a new one is closed at once.
  let open: { page: WebSocketRoute; server: WebSocketRoute } | undefined;
  let held = false;
  let away = false;
  // What her page says to come into the room, and the binary frames it is sent after each.
  const joins: { said: string; frames: Buffer[] }[] = [];
  await second.routeWebSocket(/\/ws$/, (socket) => {
    if (away) {
      void socket.close();
      return;
    }
    const server = socket.connectToServer();
    socket.onMessage((message) => {
      if (typeof message === 'string' && message.includes('JOIN_DOC')) {
        joins.push({ said: message, frames: [] });
      }
      if (!held) server.send(message);
    });
    server.onMessage((message) => {
      if (typeof message !== 'string') joins.at(-1)?.frames.push(message);
      if (!held) socket.send(message);
    });
    open = { page: socket, server };
  });
  await second.goto(address);
  await signInOnPage(second, bora);
  await ready(first);
  await ready(second);

  await editor(first).click();
  await first.keyboard.type('Shared line: one two three.');
  await first.keyboard.press('Enter');
  await first.keyboard.type('Bora:');
  await expect(editor(second).locator('p')).toHaveText(['Shared line: one two three.', 'Bora:']);
  await editor(second).locator('p').nth(1).click();
  await second.keyboard.press('End');
  await second.keyboard.type(' before the tunnel.');
  await expect(editor(first).locator('p').nth(1)).toHaveText('Bora: before the tunnel.');
  await expect(carets(first)).toHaveCount(1);

  // The line goes quiet. What Bora writes and deletes now reaches nobody yet.
  // She deletes a word both hold, and writes on.
  held = true;
  for (const _letter of 'tunnel.') await second.keyboard.press('Backspace');
  await second.keyboard.type('cave. In the tunnel.');
  await expect(editor(second).locator('p')).toHaveText([
    'Shared line: one two three.',
    'Bora: before the cave. In the tunnel.',
  ]);
  // Then it drops, and stays away for five seconds: the body can be read, not written.
  away = true;
  await open?.page.close();
  const body = second.getByTestId('card-body');
  await expect(body).toHaveAttribute('data-room', 'offline');
  await expect(second.getByTestId('body-room-note')).toContainText('The connection is lost');
  await expect(body.locator('[contenteditable="true"]')).toHaveCount(0);
  // Bora's caret is gone for Ada, who goes on writing.
  await expect(carets(first)).toHaveCount(0);
  await editor(first).locator('p').first().click();
  await first.keyboard.press('End');
  await first.keyboard.type(' Ada meanwhile.');
  await second.waitForTimeout(5000);
  // Nothing of the other's reached either.
  await expect(editor(first).locator('p').nth(1)).toHaveText('Bora: before the tunnel.');
  await expect(editor(second).locator('p').first()).toHaveText('Shared line: one two three.');

  // Back: Bora's page joins again by itself, and what both wrote is in both.
  held = false;
  away = false;
  let together = [
    'Shared line: one two three. Ada meanwhile.',
    'Bora: before the cave. In the tunnel.',
  ];
  await expect(body).toHaveAttribute('data-editor', 'ready', { timeout: 40_000 });
  for (const page of [first, second]) {
    await expect(editor(page).locator('p')).toHaveText(together, { timeout: 15_000 });
    await expect(page.getByTestId('body-lost')).toHaveCount(0);
  }
  await expect(second.getByTestId('body-room-note')).toHaveCount(0);
  // She came in the first time holding nothing, and was sent the whole (a sync step 2); the
  // second time she named the document she held, and was sent the server's state vector (a
  // step 1) and then what she lacked. (A frame: room, kind, epoch, kind of sync message.)
  const [firstJoin, again] = [joins[0], joins.at(-1)];
  expect(firstJoin?.said).not.toContain('stateVector');
  expect(firstJoin?.frames[0]?.[3]).toBe(1);
  expect(JSON.parse(again?.said ?? '{}')).toMatchObject({ epoch: 1, stateVector: /^[\w+/]+=*$/ });
  expect(again?.frames.slice(0, 2).map((frame) => frame[3])).toEqual([0, 1]);
  // Each sees the other again: Ada's caret is told to who came in, Bora's once she is in the body.
  await expect(carets(second)).toHaveCount(1);
  await editor(second).locator('p').nth(1).click();
  await expect(carets(first)).toHaveCount(1);
  await second.keyboard.press('End');
  await second.keyboard.type(' Out again.');
  together = [together[0] ?? '', `${together[1] ?? ''} Out again.`];
  await expect(editor(first).locator('p')).toHaveText(together);
  await picture(first, 'after-the-tunnel');

  // The card holds it, and both read it again after a reload.
  await expect
    .poll(
      async () => {
        const card = (await (
          await api.get(`/api/v1/w/${board.workspaceId}/cards/${cardId}`)
        ).json()) as Schemas['Card'];
        return JSON.stringify(card.body);
      },
      { timeout: 15_000 },
    )
    .toContain('Out again.');
  for (const page of [first, second]) {
    await page.reload();
    await ready(page);
    await expect(editor(page).locator('p')).toHaveText(together);
    await expect(page.locator('[data-problem]')).toHaveCount(0);
  }
  await first.context().close();
  await second.context().close();
  await api.dispose();
  expect(watched.flat()).toEqual([]);
});
