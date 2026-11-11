/**
 * What an anonymous reader of a shared board gets on the real server: every
 * mention of a body with an empty `userId`. The web app's validator draws
 * such a body (`shown`) and refuses to write it, as the server refuses it;
 * the app's screens of a card name somebody not known "Unknown user". The
 * app has no screen for an anonymous reader (a shared board by its read
 * token), so the body is taken from the server's answer here.
 */
import { expect, request, test } from '@playwright/test';

import { validateDoc } from '../src/lib/editor/schema.ts';

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

test('a body read with a read token alone names nobody in its mentions, is drawn, and is not written back', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('sharer');
  await register(base, owner);
  const api = await signedIn(base, owner);
  const me = (await (await api.get('/api/v1/users/me')).json()) as Schemas['User'];
  const board = await makeBoard(api, {
    title: 'Shared',
    cards: [{ title: 'Ask around', status: 'To do' }],
  });
  const cardId = board.cards['Ask around'] ?? '';
  const card = (await (await api.get(`/api/v1/cards/${cardId}`)).json()) as Schemas['Card'];
  const written = await api.patch(`/api/v1/cards/${cardId}`, {
    data: {
      readAt: card.updateAt,
      body: {
        type: 'doc',
        content: [
          {
            type: 'paragraph',
            content: [
              { type: 'text', text: 'ask ' },
              { type: 'mention', attrs: { userId: me.id } },
            ],
          },
        ],
      },
    },
  });
  expect(written.ok()).toBe(true);
  const shared = (await (
    await api.post(`/api/v1/boards/${board.boardId}/sharing`, { data: { enabled: true } })
  ).json()) as Schemas['Sharing'];
  expect(shared.token).toBeTruthy();

  // Read with the read token alone, signed in as nobody.
  const anonymous = await request.newContext({ baseURL: base });
  const [block] = (await (
    await anonymous.get(`/api/v1/boards/${board.boardId}/blocks`, {
      params: { readToken: shared.token ?? '', blockId: cardId },
    })
  ).json()) as Schemas['Block'][];
  await anonymous.dispose();
  const body = block?.body;
  expect(JSON.stringify(body)).toContain('"userId":""');

  // The web app draws it and would not write it; the server refuses it too.
  expect(validateDoc(body, { shown: true })).toEqual([]);
  expect(validateDoc(body)).not.toEqual([]);
  const now = (await (await api.get(`/api/v1/cards/${cardId}`)).json()) as Schemas['Card'];
  const back = await api.patch(`/api/v1/cards/${cardId}`, {
    data: { readAt: now.updateAt, body },
  });
  expect(back.status()).toBe(400);
  expect(((await back.json()) as { field?: string }).field).toBe('body');

  // The owner's own page names the owner: the mention is a user's there.
  const errors = watchErrors(page);
  await page.goto(`/board/${board.boardId}/${board.viewId}/${cardId}`);
  await signInOnPage(page, owner);
  await expect(page.getByTestId('card-body').locator('[data-mention]')).toHaveText(
    `@${owner.username}`,
  );
  await api.dispose();
  expect(errors).toEqual([]);
});
