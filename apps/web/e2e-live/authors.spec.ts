/** A departed writer is named in an earlier comment revision, but never offered as a member. */
import { expect, test, type APIRequestContext } from '@playwright/test';

import {
  ADMIN,
  boardPath,
  makeBoard,
  person,
  register,
  signedIn,
  signInOnPage,
  watchErrors,
  type Schemas,
} from './support.ts';

test.use({ locale: 'en-US' });

const body = (text: string) => ({
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
});

async function revise(api: APIRequestContext, path: string, text: string): Promise<void> {
  const split = path.lastIndexOf('/');
  const read = await api.get(path.slice(0, split), { params: { blockId: path.slice(split + 1) } });
  expect(read.ok(), await read.text()).toBe(true);
  const [current] = (await read.json()) as Schemas['Block'][];
  if (!current) throw new Error('the comment was not returned');
  const response = await api.patch(path, { data: { body: body(text), readAt: current.updateAt } });
  expect(response.ok(), await response.text()).toBe(true);
}

test('a departed writer of an earlier revision is named when the comment history opens', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('current-author');
  const earlier = person('earlier-author');
  for (const who of [owner, earlier]) await register(base, who);
  const api = await signedIn(base, owner);
  const writer = await signedIn(base, earlier);
  const writerId = ((await (await writer.get('/api/v1/users/me')).json()) as Schemas['User']).id;
  const board = await makeBoard(api, {
    title: 'Kept revisions',
    cards: [{ title: 'A conversation', status: 'To do' }],
  });
  const prefix = `/api/v1/w/${board.workspaceId}/boards/${board.boardId}`;
  const added = await api.post(`${prefix}/members`, {
    data: { userId: writerId, schemeAdmin: true },
  });
  expect(added.ok(), await added.text()).toBe(true);
  const posted = await api.post(`${prefix}/blocks`, {
    data: [
      {
        boardId: board.boardId,
        parentId: board.cards['A conversation'],
        type: 'comment',
        title: '',
        fields: {},
        createAt: 1,
        updateAt: 1,
        body: body('Original words'),
      },
    ],
  });
  expect(posted.ok(), await posted.text()).toBe(true);
  const [comment] = (await posted.json()) as Schemas['Block'][];
  const path = `${prefix}/blocks/${comment?.id ?? ''}`;
  await revise(writer, path, 'Words from the departed writer');
  await revise(api, path, 'Current words');
  const admin = await signedIn(base, ADMIN);
  const removed = await admin.delete(`/api/v1/w/${board.workspaceId}/members/${writerId}`);
  expect(removed.ok(), await removed.text()).toBe(true);
  const listed = (await (
    await api.get(`/api/v1/w/${board.workspaceId}/users`, {
      params: { search: earlier.username },
    })
  ).json()) as Schemas['User'][];
  expect(listed).toEqual([]);
  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId, board.cards['A conversation']));
  await signInOnPage(page, owner);
  await page.getByRole('button', { name: /^What the comment from .* said before$/ }).click();
  const version = page.getByTestId('comment-history').locator('[data-version="2"]');
  await expect(version).toContainText(`Changed by ${earlier.username}`);
  await expect(version).toContainText('Words from the departed writer');
  await expect(page.getByTestId('comment-history')).not.toContainText('Unknown user');
  expect(errors).toEqual([]);
  for (const client of [api, writer, admin]) await client.dispose();
});
