/**
 * What the references on screen name, as the workspace holds it: asked once
 * and together, kept current by the changes of the boards, and never a
 * title for what the reader may not see.
 */
import { describe, expect, it, vi } from 'vitest';

import { referenceTitles } from '$lib/state/index.ts';

import { openFirstBoard, settle } from './support.ts';

const UNKNOWN = '01930000-0000-7000-8000-00000000dead';

async function opened() {
  const made = await openFirstBoard();
  const [alpha, beta] = ['Alpha', 'Beta'].map((title) =>
    made.board.realCards.find((card) => card.title === title),
  );
  if (!alpha || !beta) throw new Error('no cards');
  return { ...made, alpha, beta, titles: referenceTitles(made.workspace) };
}

describe('the titles of what references name', () => {
  it('are asked once for all that is drawn in a turn, and held', async () => {
    const { backend, workspace, alpha, beta, titles } = await opened();
    const asked = vi.spyOn(backend.references, 'resolve');
    expect(referenceTitles(workspace)).toBe(titles);

    expect(titles.of(alpha.id)).toBeUndefined();
    expect(titles.of(beta.id)).toBeUndefined();
    expect(titles.of(UNKNOWN)).toBeUndefined();
    expect(titles.of(alpha.id)).toBeUndefined();
    await settle();
    expect(asked).toHaveBeenCalledOnce();
    expect(asked.mock.calls[0]?.[1]).toEqual([alpha.id, beta.id, UNKNOWN]);
    expect(titles.of(alpha.id)).toMatchObject({ id: alpha.id, kind: 'card', title: 'Alpha' });
    expect(titles.of(beta.id)?.title).toBe('Beta');
    // What is not there and what the reader may not see are the same nothing.
    expect(titles.of(UNKNOWN)).toBeNull();
    await settle();
    expect(asked).toHaveBeenCalledOnce();
  });

  it('follow a card that is renamed, and one that is deleted', async () => {
    const { other, board, alpha, beta, titles } = await opened();
    titles.of(alpha.id);
    titles.of(beta.id);
    await settle();

    await other.patchCard(board.boardId, alpha.id, { title: 'Renamed', icon: '🚀' });
    await settle();
    expect(titles.of(alpha.id)).toMatchObject({ title: 'Renamed', icon: '🚀' });

    await other.deleteBlock(board.boardId, beta.id);
    await settle();
    expect(titles.of(beta.id)).toBeNull();
  });

  it('say whether the reader may see what an identifier names', async () => {
    const { alpha, titles } = await opened();
    expect(await titles.seen(alpha.id)).toBe(true);
    expect(await titles.seen(UNKNOWN)).toBe(false);
  });

  it('offer the cards of the board changed last before anything is typed, and ask once the typing rests', async () => {
    const { backend, board, titles } = await opened();
    const asked = vi.spyOn(backend.references, 'search');
    const unasked = await titles.search('', board);
    expect(unasked.map((linked) => linked.title).sort()).toEqual(['Alpha', 'Beta', 'Gamma']);
    expect(asked).not.toHaveBeenCalled();

    let found: string | undefined;
    vi.useFakeTimers();
    try {
      const first = titles.search('a', board);
      const second = titles.search('al', board);
      await vi.runAllTimersAsync();
      expect((await second).map((linked) => linked.title)).toEqual(['Alpha']);
      found = (await second)[0]?.id;
      // The earlier term is answered as the last one typed.
      expect(await first).toEqual(await second);
      expect(asked).toHaveBeenCalledOnce();
      expect(asked.mock.calls[0]?.[1]).toBe('al');
    } finally {
      vi.useRealTimers();
    }
    // What was offered is known without being asked again, typed for or not.
    const resolved = vi.spyOn(backend.references, 'resolve');
    expect(titles.of(found ?? '')?.title).toBe('Alpha');
    for (const linked of unasked) expect(titles.of(linked.id)?.title).toBe(linked.title);
    expect(resolved).not.toHaveBeenCalled();
  });

  it('tell what names a card, and hold what was told', async () => {
    const { backend, board, alpha, beta, titles } = await opened();
    const now = await backend.card(board.boardId, beta.id);
    await backend.writeCardBody(board.boardId, beta.id, {
      body: {
        type: 'doc',
        content: [{ type: 'paragraph', content: [{ type: 'reference', attrs: { id: alpha.id } }] }],
      },
      readAt: now.updateAt,
    });
    const naming = await titles.to(alpha.id);
    expect(naming.map((linked) => linked.title)).toEqual(['Beta']);
    const resolved = vi.spyOn(backend.references, 'resolve');
    expect(titles.of(beta.id)?.title).toBe('Beta');
    expect(resolved).not.toHaveBeenCalled();
  });
});
