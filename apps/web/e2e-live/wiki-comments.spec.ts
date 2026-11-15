/**
 * The comments on a wiki's page and the versions of its body on the real
 * server (`getPageComments` and the rest, `getPageBodyVersions` and the
 * rest): two people on one page, the comments of each arriving at the other
 * as they are written, changed and deleted; and a version restored by one
 * becoming the body both see.
 */
import { expect, test, type Browser, type Page } from '@playwright/test';

import {
  makeBoard,
  person,
  register,
  type Schemas,
  signedIn,
  signInOnPage,
  watchErrors,
} from './support.ts';

test.use({ locale: 'en-US' });

const paragraph = (text: string): object => ({
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
});

/**
 * An owner's wiki with one page, a colleague on it with `role`, the page's
 * body written whole once for each of `bodies`; and each of the two on the
 * page in a browser of their own.
 */
async function twoOnAPage(
  browser: Browser,
  base: string,
  plan: { prefix: string; role: 'commenter' | 'editor'; bodies: string[] },
) {
  const owner = person(`${plan.prefix}-owner`);
  const colleague = person(`${plan.prefix}-colleague`);
  await register(base, owner);
  await register(base, colleague);
  const api = await signedIn(base, owner);
  const { workspaceId } = await makeBoard(api, { title: 'Roadmap' });
  const inWorkspace = `/api/v1/w/${workspaceId}`;
  const wiki = (await (
    await api.post(`${inWorkspace}/wikis`, { data: { title: 'Handbook', type: 'P' } })
  ).json()) as Schemas['Board'];
  const [found] = (await (
    await api.get(`${inWorkspace}/users`, { params: { search: colleague.username } })
  ).json()) as Schemas['User'][];
  await api.post(`${inWorkspace}/boards/${wiki.id}/members`, {
    data: {
      userId: found?.id,
      schemeViewer: true,
      schemeCommenter: true,
      schemeEditor: plan.role === 'editor',
    },
  });
  const pages = `${inWorkspace}/wikis/${wiki.id}/pages`;
  let made = (await (
    await api.post(pages, { data: { title: 'Notes' } })
  ).json()) as Schemas['Page'];
  for (const body of plan.bodies) {
    made = (await (
      await api.patch(`${pages}/${made.id}`, {
        data: { body: paragraph(body), readAt: made.updateAt },
      })
    ).json()) as Schemas['Page'];
  }
  const path = `/w/${workspaceId}/wikis/${wiki.id}/${made.id}`;
  const [first, second] = await Promise.all(
    [owner, colleague].map(async (who) => {
      const page = await (await browser.newContext({ locale: 'en-US' })).newPage();
      await page.goto(path);
      await signInOnPage(page, who);
      // An editor writes in the body; a commenter reads it.
      const editor = who === owner || plan.role === 'editor' ? 'ready' : 'reading';
      await expect(page.getByTestId('page-body')).toHaveAttribute('data-editor', editor);
      return page;
    }),
  );
  if (!first || !second) throw new Error('two pages');
  return { api, first, second, page: `${pages}/${made.id}` };
}

const rows = (page: Page) => page.getByTestId('card-comments').locator('[data-comment]');

async function writeComment(page: Page, text: string): Promise<void> {
  const composer = page.getByTestId('comment-composer');
  await composer.getByTestId('comment-field').fill(text);
  await composer.getByTestId('comment-send').click();
}

test('comments on a page arrive at the other person as they are written, changed and deleted', async ({
  browser,
  baseURL,
}) => {
  const { api, first, second, page } = await twoOnAPage(browser, baseURL ?? '', {
    prefix: 'page-comments',
    role: 'commenter',
    bodies: ['The plan'],
  });
  const watched = [watchErrors(first), watchErrors(second)];

  await writeComment(second, 'From the commenter');
  await expect(rows(first)).toHaveCount(1);
  await expect(rows(first).first()).toContainText('From the commenter');
  await writeComment(first, 'From the owner');
  await expect(rows(second)).toHaveCount(2);
  await expect(rows(second).last()).toContainText('From the owner');
  // The commenter changes their own comment, not the owner's.
  const own = rows(second).first();
  await expect(rows(second).last().locator('[data-action="edit"]')).toHaveCount(0);
  await own.locator('[data-action="edit"]').click();
  const field = own.getByRole('textbox', { name: 'Comment text' });
  await field.fill('From the commenter, changed');
  await field.press('Control+Enter');
  await expect(rows(first).first()).toContainText('From the commenter, changed');
  await expect(rows(first).first().getByTestId('comment-edited')).toBeVisible();
  await rows(first).first().locator('[data-action="history"]').click();
  await expect(rows(first).first().getByTestId('comment-history')).toContainText(
    'From the commenter',
  );
  // The server keeps them as they are shown.
  const stored = (await (await api.get(`${page}/comments`)).json()) as Schemas['Block'][];
  expect(stored.map((comment) => comment.body)).toEqual([
    paragraph('From the commenter, changed'),
    paragraph('From the owner'),
  ]);
  await own.getByRole('button', { name: /^Delete the comment from/ }).click();
  await expect(rows(first)).toHaveCount(1);
  await expect(rows(first).first()).toContainText('From the owner');
  // Read again, they are as the server has them.
  await second.reload();
  await expect(rows(second)).toHaveCount(1);
  for (const errors of watched) expect(errors).toEqual([]);
  await first.context().close();
  await second.context().close();
  await api.dispose();
});

test('a version of the page restored by one is the body both see', async ({ browser, baseURL }) => {
  const { api, first, second, page } = await twoOnAPage(browser, baseURL ?? '', {
    prefix: 'page-versions',
    role: 'editor',
    bodies: ['The first plan', 'The second plan'],
  });
  const watched = [watchErrors(first), watchErrors(second)];
  await expect(first.getByTestId('page-body')).toContainText('The second plan');

  await first.getByTestId('page-menu').click();
  await first.locator('[data-action="versions"]').click();
  const versions = first.getByTestId('body-versions');
  const listed = versions.locator('[data-version]');
  await expect(listed).toHaveCount(2);
  await listed.nth(1).click();
  await expect(first.getByTestId('body-version')).toContainText('The first plan');
  await first.getByTestId('body-version-restore').click();
  await first.getByTestId('confirm-dialog').getByRole('button', { name: 'Restore' }).click();
  await expect(versions).toBeHidden();
  for (const each of [first, second]) {
    await expect(each.getByTestId('page-body')).toContainText('The first plan');
    await expect(each.getByTestId('page-body')).not.toContainText('The second plan');
  }
  const stored = (await (await api.get(page)).json()) as Schemas['Page'];
  expect(JSON.stringify(stored.body)).toContain('The first plan');
  // The body it replaced is kept as a version: three now.
  const kept = (await (await api.get(`${page}/body-versions`)).json()) as Schemas['BodyVersion'][];
  expect(kept.length).toBe(3);
  for (const errors of watched) expect(errors).toEqual([]);
  await first.context().close();
  await second.context().close();
  await api.dispose();
});
