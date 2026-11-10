/**
 * Formulas and diagrams in the assembled app: the body of a card draws them
 * in the editor (a board the reader may change) and in the read-only view (a
 * board the reader may only look at), and both run under the page's content
 * security policy without a refusal.
 *
 * What the two libraries may and may not do with hostile input is tested
 * where it can be tested widely: spikes/editor/attacks.spec.ts and the unit
 * tests. Here is only that the assembled app, with the policy, draws them.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

/** What `window.fvociDev` offers a test (src/app/backend.ts). Only what is used here. */
interface DevBackend {
  teams(): Promise<{ id: string }[]>;
  boards(teamId: string): Promise<{ id: string; title: string }[]>;
  views(boardId: string): Promise<{ id: string; title: string }[]>;
  cards(boardId: string): Promise<{ id: string; title: string }[]>;
  card(boardId: string, cardId: string): Promise<{ updateAt: number }>;
  writeCardBody(boardId: string, cardId: string, write: object): Promise<unknown>;
  patchSettings(patch: object): Promise<unknown>;
}
interface Dev {
  backend: DevBackend;
  someoneElse(): DevBackend;
}

const BODY = {
  type: 'doc',
  content: [
    {
      type: 'paragraph',
      content: [
        { type: 'text', text: 'Energy: ' },
        { type: 'inlineMath', attrs: { latex: 'E = mc^2' } },
      ],
    },
    { type: 'blockMath', attrs: { latex: '\\int_0^1 x^2\\,dx = \\frac{1}{3}' } },
    {
      type: 'codeBlock',
      attrs: { language: 'mermaid' },
      content: [
        { type: 'text', text: 'flowchart LR\n  A[Start] --> B{Choice}\n  B -->|yes| C[End]' },
      ],
    },
  ],
};

/**
 * Opens the first card of a board, and then somebody else gives it this body.
 * In that order: the data lives in memory and a page that loads starts from
 * the seed again, so what was written before the card was opened is gone. It
 * is written as another member because on the board the reader may only look
 * at, the reader cannot.
 */
async function openCardWithEmbeds(page: Page, board: string): Promise<Locator> {
  await page.goto('/');
  await expect(page.getByTestId('board-page')).toBeVisible();
  const place = await page.evaluate(async (boardTitle) => {
    const reader = (window as unknown as { fvociDev: Dev }).fvociDev.backend;
    const [team] = await reader.teams();
    const boards = await reader.boards(team?.id ?? '');
    const found = boards.find((each) => each.title.startsWith(boardTitle));
    if (!found) throw new Error(`no board ${boardTitle}`);
    const [card] = await reader.cards(found.id);
    const [view] = await reader.views(found.id);
    if (!card || !view) throw new Error('the board has no card or no view');
    return { boardId: found.id, viewId: view.id, cardId: card.id };
  }, board);
  await page.goto(`/board/${place.boardId}/${place.viewId}/${place.cardId}`);
  const body = page.getByTestId('card-body');
  await expect(body).toBeVisible();
  await expect(page.getByTestId('card-comments')).toBeVisible();
  await page.evaluate(
    async ([at, doc]) => {
      const handle = (window as unknown as { fvociDev: Dev }).fvociDev;
      const { updateAt } = await handle.backend.card(at.boardId, at.cardId);
      await handle
        .someoneElse()
        .writeCardBody(at.boardId, at.cardId, { body: doc, readAt: updateAt });
    },
    [place, BODY] as const,
  );
  return body;
}

function refusals(page: Page): string[] {
  const seen: string[] = [];
  page.on('console', (message) => {
    if (message.text().includes('Content Security Policy')) seen.push(message.text());
  });
  return seen;
}

async function drawn(body: Locator): Promise<void> {
  // The inline formula and the one on its own line.
  await expect(body.locator('.doc-math[data-state="drawn"]')).toHaveCount(2, { timeout: 30_000 });
  await expect(body.locator('.doc-math .katex').first()).toBeVisible();
  await expect(body.locator('.doc-math[data-state="error"]')).toHaveCount(0);
  const diagram = body.locator('.doc-diagram[data-state="drawn"]');
  await expect(diagram).toHaveCount(1, { timeout: 30_000 });
  await expect(diagram.locator('svg')).toBeVisible();
  await expect(diagram.locator('svg')).toContainText('Start');
}

test.describe.configure({ timeout: 90_000 });

test('the editor of a card draws a formula and a diagram, under the policy', async ({ page }) => {
  const seen = refusals(page);
  const body = await openCardWithEmbeds(page, 'Welcome');
  await expect(body).toHaveAttribute('data-editor', 'ready');
  await drawn(body);
  expect(seen).toEqual([]);
});

test('a card the reader may only look at draws them too, without the editor', async ({ page }) => {
  const seen = refusals(page);
  const body = await openCardWithEmbeds(page, 'Company Goals');
  await expect(body).toHaveAttribute('data-editor', 'none');
  await expect(body.locator('[contenteditable="true"]')).toHaveCount(0);
  await drawn(body);
  expect(seen).toEqual([]);
});

test('on the dark screen the diagram is drawn again with the colours of the dark screen', async ({
  page,
}) => {
  const body = await openCardWithEmbeds(page, 'Company Goals');
  await drawn(body);
  const fillOf = (): Promise<string> =>
    body
      .locator('.doc-diagram svg text, .doc-diagram svg tspan')
      .first()
      .evaluate((element) => getComputedStyle(element).fill);
  const light = await fillOf();
  // The way the settings dialog does it: the reader's account says dark.
  await page.evaluate(() =>
    (window as unknown as { fvociDev: Dev }).fvociDev.backend.patchSettings({ brightness: 'dark' }),
  );
  await expect(page.locator('html')).toHaveClass(/dark/);
  // The diagram watches the class of the page and is drawn again.
  await expect.poll(fillOf, { timeout: 30_000 }).not.toBe(light);
  await drawn(body);
});
