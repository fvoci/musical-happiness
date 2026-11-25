/**
 * View, card, comment, attachment, and block HTTP operations for `ServerBackend`
 * (`api/backend.ts`), including 1-round-trip `patchView` & `patchComment` (§1.3.5).
 */
import type { CardPreview } from '../editor/card-preview.ts';
import type { DocNode } from '../editor/schema.ts';
import { createBoardView as emptyViewOf, type BlockPatch, type BoardView } from '../logic/wire.ts';
import {
  BackendError,
  isViewBlock,
  refusedMember,
  type Attachment,
  type BodyWrite,
  type CardDetail,
  type CardDraft,
  type CardPatch,
  type CardSummary,
  type CardTelling,
  type Change,
  type Comment,
  type CommentVersion,
  type UploadedFile,
  type ViewDraft,
} from '../state/backend.ts';
import { pagesTogether } from '../state/page-reads.ts';
import { applyViewPatch } from '../state/patches.ts';
import type { BlockTurns } from '../state/write.ts';

import {
  attachmentOf,
  cardDetailOf,
  cardOf,
  commentOf,
  commentVersionOf,
  newCardRequest,
  patchCardRequest,
  previewsOf,
  viewOf,
  wireBody,
  type Schemas,
} from './convert.ts';
import type { RequestOptions } from './http.ts';
import type { WorkspacePath } from './requests.ts';

type ChangedBlock = Extract<Change, { kind: 'blocks' }>['blocks'][number];

export type InWorkspaceFn = <T>(
  path: WorkspacePath,
  params?: Record<string, string>,
  options?: RequestOptions,
  workspaceId?: string,
) => Promise<T>;

export class ServerBlocks {
  readonly #inWorkspace: InWorkspaceFn;
  readonly #turns: BlockTurns;
  readonly #knownViews = new Map<string, BoardView>();
  readonly #knownComments = new Map<string, Comment>();

  constructor(inWorkspace: InWorkspaceFn, turns: BlockTurns) {
    this.#inWorkspace = inWorkspace;
    this.#turns = turns;
  }

  learnView(view: BoardView): void {
    this.#knownViews.set(view.id, view);
  }

  learnLiveBlock(block: ChangedBlock): void {
    if (isViewBlock(block)) this.#knownViews.set(block.id, block);
    else if (block.type === 'comment' && 'body' in block) {
      this.#knownComments.set(block.id, block);
    }
  }

  async views(boardId: string): Promise<BoardView[]> {
    const blocks = await this.#inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { type: 'view' } },
    );
    const views = blocks.map(viewOf);
    for (const view of views) this.#knownViews.set(view.id, view);
    return views;
  }

  async createView(boardId: string, draft: ViewDraft): Promise<BoardView> {
    const block: Schemas['NewBlock'] = {
      containerId: boardId,
      type: 'view',
      title: draft.title,
      fields: draft.fields,
      createdAt: 1,
      updatedAt: 1,
    };
    const [made] = await this.#inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { detail: 'the server made no view' });
    const view = viewOf(made);
    this.#knownViews.set(view.id, view);
    return view;
  }

  patchView(boardId: string, viewId: string, patch: BlockPatch): Promise<BoardView> {
    return this.#turns.patchView(viewId, patch, (sent, conditionalQueued) =>
      this.#patchView(boardId, viewId, sent, conditionalQueued),
    );
  }

  async #patchView(
    boardId: string,
    viewId: string,
    patch: BlockPatch,
    conditionalQueued: boolean,
  ): Promise<{ before: BoardView; after: BoardView }> {
    const known =
      conditionalQueued && patch.readAt === undefined
        ? await this.#view(boardId, viewId)
        : (this.#knownViews.get(viewId) ?? {
            ...emptyViewOf(),
            id: viewId,
            containerId: boardId,
            updatedAt: patch.readAt ?? 0,
          });
    const answered = await this.#inWorkspace<Schemas['Block'] | undefined>(
      '/boards/{boardId}/blocks/{blockId}',
      { boardId, blockId: viewId },
      { method: 'PATCH', json: patch },
    );
    let after: BoardView;
    if (answered) {
      after = viewOf(answered);
    } else if (conditionalQueued) {
      after = await this.#view(boardId, viewId).catch(() =>
        applyViewPatch(this.#knownViews.get(viewId) ?? known, patch),
      );
    } else {
      after = applyViewPatch(this.#knownViews.get(viewId) ?? known, patch);
    }
    this.#knownViews.set(viewId, after);
    return { before: known, after };
  }

  async #view(boardId: string, viewId: string): Promise<BoardView> {
    const [view] = await this.#inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { blockId: viewId } },
    );
    if (view?.type !== 'view') {
      throw new BackendError(404, { detail: 'no such view on this board' });
    }
    return viewOf(view);
  }

  async cards(boardId: string): Promise<CardSummary[]> {
    return (await this.#everyCard(boardId, {})).map(cardOf);
  }

  async cardPreviews(boardId: string): Promise<Map<string, CardPreview>> {
    return previewsOf(await this.#everyCard(boardId, { preview: 'true' }));
  }

  #everyCard(boardId: string, query: Record<string, string>): Promise<Schemas['Card'][]> {
    return pagesTogether((paging) =>
      this.#inWorkspace<Schemas['Card'][]>(
        '/boards/{boardId}/cards',
        { boardId },
        { query: { ...paging, ...query } },
      ),
    );
  }

  async card(boardId: string, cardId: string): Promise<CardDetail> {
    const card = await this.#inWorkspace<Schemas['Card']>('/cards/{cardId}', { cardId, boardId });
    if (card.boardId !== boardId) {
      throw new BackendError(404, { detail: 'no such card on this board' });
    }
    return cardDetailOf(card);
  }

  async createCard(boardId: string, draft: CardDraft): Promise<CardSummary> {
    const card = await this.#inWorkspace<Schemas['Card']>(
      '/boards/{boardId}/cards',
      { boardId },
      newCardRequest(draft),
    );
    return cardOf(card);
  }

  patchCard(
    boardId: string,
    cardId: string,
    patch: CardPatch,
    telling?: CardTelling,
  ): Promise<CardSummary> {
    return this.#turns.run(cardId, async () =>
      cardOf(
        await this.#inWorkspace<Schemas['Card']>(
          '/cards/{cardId}',
          { cardId, boardId },
          patchCardRequest(patch, telling),
        ),
      ),
    );
  }

  async comments(boardId: string, cardId: string): Promise<Comment[]> {
    const list = (await this.#children(boardId, cardId, 'comment')).map(commentOf);
    for (const comment of list) this.#knownComments.set(comment.id, comment);
    return list;
  }

  async createComment(boardId: string, cardId: string, body: DocNode): Promise<Comment> {
    const block = {
      containerId: boardId,
      parentId: cardId,
      type: 'comment',
      title: '',
      fields: {},
      createdAt: 1,
      updatedAt: 1,
      body: wireBody(body),
    } satisfies Schemas['NewBlock'] & { body: Schemas['Body'] };
    const [made] = await this.#inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { detail: 'the server made no comment' });
    const comment = commentOf(made);
    this.#knownComments.set(comment.id, comment);
    return comment;
  }

  patchComment(boardId: string, commentId: string, write: BodyWrite): Promise<Comment> {
    const body = write.body;
    if (!body) {
      return Promise.reject(
        new BackendError(400, {
          detail: 'a comment needs a text',
          errors: [refusedMember('body', 'a comment needs a text', 'required')],
        }),
      );
    }
    return this.#turns.run(commentId, async () => {
      const patch: Schemas['BlockPatch'] = { body: wireBody(body), readAt: write.readAt };
      const answered = await this.#inWorkspace<Schemas['Block'] | undefined>(
        '/boards/{boardId}/blocks/{blockId}',
        { boardId, blockId: commentId },
        { method: 'PATCH', json: patch },
      );
      if (answered) {
        const comment = commentOf(answered);
        this.#knownComments.set(commentId, comment);
        return comment;
      }
      const before = this.#knownComments.get(commentId);
      const updated: Comment = before
        ? { ...before, body, updatedAt: Math.max(before.updatedAt, write.readAt) }
        : {
            id: commentId,
            containerId: boardId,
            parentId: '',
            type: 'comment',
            title: '',
            fields: {},
            body,
            createdBy: '',
            modifiedBy: '',
            createdAt: write.readAt,
            updatedAt: write.readAt,
          };
      this.#knownComments.set(commentId, updated);
      return updated;
    });
  }

  async commentVersions(boardId: string, commentId: string): Promise<CommentVersion[]> {
    const versions = await this.#inWorkspace<Schemas['CommentVersion'][]>(
      '/boards/{boardId}/blocks/{blockId}/versions',
      { boardId, blockId: commentId },
    );
    return versions.map(commentVersionOf);
  }

  async attachments(boardId: string, cardId: string): Promise<Attachment[]> {
    return (await this.#children(boardId, cardId, 'attachment')).map(attachmentOf);
  }

  #children(boardId: string, parentId: string, type: string): Promise<Schemas['Block'][]> {
    return this.#inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { query: { parentId, type } },
    );
  }

  async createAttachment(boardId: string, cardId: string, file: UploadedFile): Promise<Attachment> {
    const block: Schemas['NewBlock'] = {
      containerId: boardId,
      parentId: cardId,
      type: 'attachment',
      title: file.name,
      fields: { fileId: file.fileId, size: file.size, contentType: file.contentType },
      createdAt: 1,
      updatedAt: 1,
    };
    const [made] = await this.#inWorkspace<Schemas['Block'][]>(
      '/boards/{boardId}/blocks',
      { boardId },
      { method: 'POST', json: [block] },
    );
    if (!made) throw new BackendError(500, { detail: 'the server made no attachment' });
    return attachmentOf(made);
  }

  async deleteBlock(boardId: string, blockId: string): Promise<void> {
    await this.#inWorkspace(
      '/boards/{boardId}/blocks/{blockId}',
      { boardId, blockId },
      { method: 'DELETE' },
    );
  }

  async restoreBlock(boardId: string, blockId: string): Promise<void> {
    await this.#inWorkspace(
      '/boards/{boardId}/blocks/{blockId}/restore',
      { boardId, blockId },
      { method: 'POST' },
    );
  }
}
