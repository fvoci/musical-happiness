/**
 * Display names on the real server: given at registration or not, changed in
 * the settings with a refused name told beside its field, and every place
 * that shows a person showing "Display name (@login)", or the login name
 * alone without one.
 */
import { expect, test, type APIRequestContext } from '@playwright/test';

import {
  boardPath,
  invitation,
  makeBoard,
  person,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

async function me(api: APIRequestContext): Promise<Schemas['User']> {
  return (await (await api.get('/api/v1/users/me')).json()) as Schemas['User'];
}

test('a display name is given where the account is made, changed in the settings, and a refused one is told at its field', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const errors = watchErrors(page);
  const who = person('named');
  const token = await invitation(base);

  await page.goto(`/invite/${token}`);
  const form = page.getByTestId('invite-register-form');
  await form.getByLabel(/^user name$/i).fill(who.username);
  // A name that turns the text around: the server's refusal is said beside the field.
  const displayName = form.getByLabel(/^display name$/i);
  await displayName.fill(`Ada${String.fromCharCode(0x202e)}`);
  await form.getByLabel(/email/i).fill(who.email);
  await form.getByLabel(/^password$/i).fill(who.password);
  await form.getByRole('button', { name: /^create account and join$/i }).click();
  await expect(form.getByTestId('problem-displayName')).toHaveText(/turn the text around/);
  await expect(displayName).toBeFocused();
  await displayName.fill('  Ada Lovelace ');
  await form.getByRole('button', { name: /^create account and join$/i }).click();
  await expect(page.getByTestId('app-shell')).toBeVisible();

  const api = await signedIn(base, who);
  expect((await me(api)).displayName).toBe('Ada Lovelace');

  await page.getByTestId('open-settings').click();
  const settings = page.getByTestId('settings-dialog');
  await expect(settings.getByTestId('signed-in-as')).toHaveText(
    `Signed in as Ada Lovelace (@${who.username})`,
  );
  const field = settings.getByLabel(/^display name$/i);
  await expect(field).toHaveValue('Ada Lovelace');
  const save = settings.getByRole('button', { name: 'Save name' });

  // A character that shows nothing: refused by the server, said beside the field.
  await field.fill(`Ada${String.fromCharCode(0x200b)}L`);
  await save.click();
  await expect(settings.getByTestId('problem-displayName')).toHaveText(/show nothing/);
  await expect(field).toHaveAttribute('aria-invalid', 'true');
  expect((await me(api)).displayName).toBe('Ada Lovelace');
  // Too long, counted as the server counts (UTF-16 units).
  await field.fill('x'.repeat(65));
  await save.click();
  await expect(settings.getByTestId('problem-displayName')).toHaveText(/at most 64/);

  await field.fill('Countess');
  await save.click();
  await expect(settings.getByTestId('problem-displayName')).toHaveCount(0);
  await expect(settings.getByText('Saved.')).toBeVisible();
  await expect.poll(async () => (await me(api)).displayName).toBe('Countess');
  await expect(settings.getByTestId('signed-in-as')).toHaveText(
    `Signed in as Countess (@${who.username})`,
  );

  // Empty is none: the login name alone.
  await field.fill('');
  await save.click();
  await expect.poll(async () => (await me(api)).displayName).toBe('');
  await expect(settings.getByTestId('signed-in-as')).toHaveText(`Signed in as ${who.username}`);

  // Kept by the account: another page reads it from the server.
  await api.patch('/api/v1/users/me', { data: { displayName: 'Lady Ada' } });
  await page.reload();
  await page.getByTestId('open-settings').click();
  await expect(settings.getByLabel(/^display name$/i)).toHaveValue('Lady Ada');
  await api.dispose();
  expect(errors).toEqual([]);
});

test('people are shown by display name and login name wherever the board names them', async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? '';
  const owner = person('host');
  const named = person('grace');
  const plain = person('plain');
  for (const who of [owner, named, plain]) await register(base, who);
  const ownerApi = await signedIn(base, owner);
  const namedApi = await signedIn(base, named);
  const plainApi = await signedIn(base, plain);
  await namedApi.patch('/api/v1/users/me', { data: { displayName: 'Grace Hopper' } });
  const namedId = (await me(namedApi)).id;
  const graceLabel = `Grace Hopper (@${named.username})`;

  const board = await makeBoard(ownerApi, {
    title: 'Named people',
    cards: [{ title: 'Plan the launch', status: 'To do' }],
    members: [{ who: named, role: 'editor' }],
  });
  const cardId = board.cards['Plan the launch'] ?? '';
  // A person property with Grace in it, a body that mentions her, and her comment.
  const patched = await ownerApi.patch(
    `/api/v1/w/${board.workspaceId}/containers/${board.boardId}`,
    {
      data: {
        updatedCardProperties: [{ id: 'owner', name: 'Owner', type: 'person', options: [] }],
      },
    },
  );
  expect(patched.ok()).toBe(true);
  const card = (await (
    await ownerApi.get(`/api/v1/w/${board.workspaceId}/cards/${cardId}`)
  ).json()) as Schemas['Card'];
  const written = await ownerApi.patch(`/api/v1/w/${board.workspaceId}/cards/${cardId}`, {
    data: {
      readAt: card.updateAt,
      updatedProperties: { owner: namedId },
      body: {
        type: 'doc',
        content: [
          {
            type: 'paragraph',
            content: [
              { type: 'text', text: 'ask ' },
              { type: 'mention', attrs: { userId: namedId } },
            ],
          },
        ],
      },
    },
  });
  expect(written.ok()).toBe(true);
  const commented = await namedApi.post(
    `/api/v1/w/${board.workspaceId}/boards/${board.boardId}/blocks`,
    {
      data: [
        {
          boardId: board.boardId,
          parentId: cardId,
          type: 'comment',
          title: '',
          createAt: 1,
          updateAt: 1,
          fields: {},
          body: {
            type: 'doc',
            content: [{ type: 'paragraph', content: [{ type: 'text', text: 'On it' }] }],
          },
        },
      ],
    },
  );
  expect(commented.ok()).toBe(true);

  const errors = watchErrors(page);
  await page.goto(boardPath(board, board.viewId, cardId));
  await signInOnPage(page, owner);
  const dialog = page.getByTestId('card-dialog');
  await expect(dialog).toBeVisible();
  // The mention, the person property and the comment's author.
  await expect(page.getByTestId('card-body').locator('[data-mention]')).toHaveText([
    `@${graceLabel}`,
  ]);
  await expect(dialog.locator('[data-slot="person-chip"]')).toHaveText([graceLabel]);
  await expect(page.getByTestId('card-comments')).toContainText(graceLabel);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);

  // The share dialog: members, and people found by either name.
  await page.getByTestId('share-board').click();
  const members = page.getByTestId('share-members');
  await expect(members.locator(`[data-member="${graceLabel}"]`)).toHaveCount(1);
  await expect(members.locator(`[data-member="${owner.username}"]`)).toHaveCount(1);
  const search = page.getByTestId('share-search');
  await plainApi.patch('/api/v1/users/me', { data: { displayName: 'Plainly Named' } });
  await search.fill('plainly');
  await expect(page.getByTestId('share-candidates')).toContainText(
    `Plainly Named (@${plain.username})`,
  );
  for (const api of [ownerApi, namedApi, plainApi]) await api.dispose();
  expect(errors).toEqual([]);
});
