/** The previews a gallery shows: read when it starts, followed by change messages, the same in both backends. */
import { describe, expect, it, vi } from 'vitest';

import { cardOfBlock } from '$lib/api/convert.ts';
import { CardPreviews } from '$lib/state/index.ts';

import { openFirstBoard, serverBackend, settle, text } from './support.ts';

describe('the previews of a gallery', () => {
  it('are read when it starts, follow a body written elsewhere, and stop with it', async () => {
    const { board, backend, other } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    if (!alpha) throw new Error('no card');
    const asked = vi.spyOn(backend, 'cardPreviews');

    const previews = new CardPreviews(backend, board.boardId);
    expect(asked).not.toHaveBeenCalled();
    const stop = previews.start();
    await settle();
    expect(asked).toHaveBeenCalledOnce();
    expect(previews.of(alpha.id)).toEqual({ imageFileId: '', text: 'first body' });

    const now = await other.card(board.boardId, alpha.id);
    await other.writeCardBody(board.boardId, alpha.id, {
      body: text('written elsewhere'),
      readAt: now.updateAt,
    });
    await settle();
    expect(previews.of(alpha.id)?.text).toBe('written elsewhere');

    stop();
    const later = await other.card(board.boardId, alpha.id);
    await other.writeCardBody(board.boardId, alpha.id, {
      body: text('not followed'),
      readAt: later.updateAt,
    });
    await settle();
    expect(previews.of(alpha.id)?.text).toBe('written elsewhere');
  });

  it('keep what a change message told over a reading begun before it', async () => {
    const { board, backend, other } = await openFirstBoard();
    const alpha = board.realCards.find((card) => card.title === 'Alpha');
    if (!alpha) throw new Error('no card');
    // The reading takes its answer before the write, and gives it after the write's message.
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const real = backend.cardPreviews.bind(backend);
    vi.spyOn(backend, 'cardPreviews').mockImplementationOnce(async (boardId) => {
      const answer = await real(boardId);
      await held;
      return answer;
    });
    const previews = new CardPreviews(backend, board.boardId);
    const stop = previews.start();
    await settle();
    const now = await other.card(board.boardId, alpha.id);
    await other.writeCardBody(board.boardId, alpha.id, {
      body: text('newer'),
      readAt: now.updateAt,
    });
    await settle();
    release();
    await settle();
    expect(previews.of(alpha.id)?.text).toBe('newer');
    stop();
  });
});

describe('the server backend', () => {
  it('asks the card list with previews, and makes one of a card block’s body', async () => {
    const sent: string[] = [];
    vi.stubGlobal('fetch', (url: string) => {
      sent.push(url);
      return Promise.resolve(
        new Response(
          JSON.stringify([
            { id: 'c1', preview: { imageFileId: 'f1', text: 'Hello' } },
            { id: 'c2', preview: { imageFileId: '', text: '' } },
            { id: 'c3' },
          ]),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      );
    });
    const previews = await serverBackend('').cardPreviews('b');
    vi.unstubAllGlobals();
    expect(sent).toEqual(['/api/v1/w/w1/boards/b/cards?perPage=0&preview=true']);
    expect([...previews]).toEqual([
      ['c1', { imageFileId: 'f1', text: 'Hello' }],
      ['c2', { imageFileId: '', text: '' }],
    ]);

    const block = cardOfBlock({
      id: 'c4',
      containerId: 'b',
      parentId: 'b',
      createdBy: 'u',
      modifiedBy: 'u',
      schema: 1,
      type: 'card',
      title: 'With a picture',
      fields: { properties: {} },
      createAt: 1,
      updateAt: 1,
      deleteAt: 0,
      body: {
        type: 'doc',
        content: [
          { type: 'image', attrs: { fileId: 'f9' } },
          { type: 'paragraph', content: [{ type: 'text', text: 'Below it' }] },
        ],
      },
    });
    expect(block.preview).toEqual({ imageFileId: 'f9', text: 'Below it' });
  });
});
