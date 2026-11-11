/**
 * The server's shapes (`contract.ts`, made from `contracts/openapi.json`)
 * and the shapes the screens work with (`lib/state/backend.ts`,
 * `lib/logic/wire.ts`), one into the other.
 *
 * Where the two differ the screens' shape stays and the difference is
 * settled here; what is still open is listed in `docs/contract-assumptions.md`.
 */
import { cardPreviewOf, type CardPreview } from '../editor/card-preview.ts';
import { countTasks, type DocNode } from '../editor/schema.ts';
import { readDisplaySettings } from '../logic/display.ts';
import { readLanguageSettings } from '../logic/language.ts';
import { readRegionalSettings } from '../logic/regional.ts';
import {
  createBoardView,
  createCard,
  type Block,
  type BlockType,
  type Board,
  type BoardMember,
  type BoardPatch,
  type BoardView,
  type PropertyTemplate,
} from '../logic/wire.ts';
import {
  BackendError,
  refusedMember,
  type AccountSettings,
  type BoardDraft,
  type AccountSettingsPatch,
  type Attachment,
  type BoardRole,
  type CardDetail,
  type CardDraft,
  type Comment,
  type CommentVersion,
  type CardSummary,
  type Category,
  type Sharing,
  type Team,
  type User,
  type ViewDraft,
} from '../state/backend.ts';

import type { components } from './contract.ts';

export type Schemas = components['schemas'];

/**
 * A user: a name to log in with and one to be shown by, and no email
 * address (it is never answered).
 */
export function userOf(user: Schemas['User']): User {
  return {
    id: user.id,
    username: user.username,
    displayName: user.displayName,
    email: '',
    isAdmin: user.isAdmin,
    createAt: user.createAt,
    updateAt: user.updateAt,
    deleteAt: user.deleteAt,
  };
}

export function teamOf(team: Schemas['Team']): Team {
  return { id: team.id, title: team.title };
}

export function boardOf(board: Schemas['Board']): Board {
  return {
    id: board.id,
    teamId: board.teamId,
    createdBy: board.createdBy,
    modifiedBy: board.modifiedBy,
    type: board.type,
    minimumRole: board.minimumRole,
    title: board.title,
    description: board.description,
    icon: board.icon,
    showDescription: board.showDescription,
    isTemplate: board.isTemplate,
    isBuiltin: board.isBuiltin,
    templateVersion: board.templateVersion,
    properties: board.properties as Board['properties'],
    cardProperties: board.cardProperties as unknown as PropertyTemplate[],
    createAt: board.createAt,
    updateAt: board.updateAt,
    deleteAt: board.deleteAt,
  };
}

/**
 * A membership. The contract's has no `minimumRole` or `synthetic` (whether
 * the membership is one by the board being open): no screen reads either.
 */
export function memberOf(member: Schemas['BoardMember']): BoardMember {
  return { ...member, minimumRole: '', synthetic: false };
}

/** A block of the API as the screens' block. Its body, if any, is the card's or comment's to carry. */
export function blockOf(block: Schemas['Block']): Block {
  const { body: _body, ...rest } = block;
  return { ...rest, type: block.type as BlockType, fields: { ...block.fields } };
}

/** The body of a block or a card, as the screens' document. */
export function bodyOf(body: Schemas['Block']['body']): DocNode | null {
  return (body ?? null) as DocNode | null;
}

export function viewOf(block: Schemas['Block']): BoardView {
  return createBoardView(blockOf(block));
}

/**
 * The card summary of a card block, as a change message brings it. The
 * block carries the whole body and no counts (`contracts/websocket.md`, G5):
 * whether there is a body and how its tasks stand are worked out from it.
 * It has no `lastCommentAt`: the board keeps the one it had.
 */
export function cardOfBlock(block: Schemas['Block']): CardSummary {
  const body = bodyOf(block.body);
  const tasks = body ? countTasks(body) : { total: 0, checked: 0 };
  return {
    ...createCard(blockOf(block)),
    hasBody: body !== null,
    tasksTotal: tasks.total,
    tasksDone: tasks.checked,
    preview: cardPreviewOf(body),
  };
}

/** A card of the cards API: its members are where a card block has its `fields`. */
export function cardOf(card: Schemas['Card']): CardSummary {
  return {
    ...createCard({
      id: card.id,
      boardId: card.boardId,
      parentId: '',
      createdBy: card.createdBy,
      modifiedBy: card.modifiedBy,
      schema: 1,
      type: 'card',
      title: card.title,
      fields: { icon: card.icon, isTemplate: card.isTemplate, properties: card.properties },
      createAt: card.createAt,
      updateAt: card.updateAt,
      deleteAt: card.deleteAt,
    }),
    hasBody: card.hasBody,
    tasksTotal: card.tasksTotal,
    tasksDone: card.tasksDone,
    lastCommentAt: card.lastCommentAt,
  };
}

/** The previews of a card list asked for them, by card. */
export function previewsOf(cards: readonly Schemas['Card'][]): Map<string, CardPreview> {
  return new Map(
    cards.flatMap((card) =>
      card.preview
        ? [[card.id, { imageFileId: card.preview.imageFileId, text: card.preview.text }]]
        : [],
    ),
  );
}

/** A card read alone, with its body (a list of cards carries none). */
export function cardDetailOf(card: Schemas['Card']): CardDetail {
  return { ...cardOf(card), body: bodyOf(card.body) };
}

/** A document as the contract's body: the contract leaves its members open. */
export function wireBody(doc: DocNode): Schemas['Body'] {
  return doc as unknown as Schemas['Body'];
}

/** A card to make. */
export function newCardOf(draft: CardDraft): Schemas['NewCard'] {
  return {
    title: draft.title ?? '',
    icon: draft.icon ?? '',
    isTemplate: draft.isTemplate ?? false,
    properties: draft.properties ?? {},
    ...(draft.body ? { body: wireBody(draft.body) } : {}),
  };
}

/**
 * A comment: a block whose text is its body. A comment stored before bodies
 * were (its text in the title, as the original app wrote it) is shown with
 * each line of the title as a paragraph of plain text.
 */
export function commentOf(block: Schemas['Block']): Comment {
  const body = bodyOf(block.body);
  const lines = block.title.trim().split(/\r?\n/);
  return {
    ...blockOf(block),
    type: 'comment',
    body:
      body ??
      (lines.join('') === ''
        ? null
        : {
            type: 'doc',
            content: lines.map((text) =>
              text
                ? { type: 'paragraph', content: [{ type: 'text', text }] }
                : { type: 'paragraph' },
            ),
          }),
  };
}

/** An earlier text of a comment. */
export function commentVersionOf(version: Schemas['CommentVersion']): CommentVersion {
  return { ...version, body: bodyOf(version.body) };
}

/**
 * An attachment block. Its file is `fields.fileId` (older blocks name it
 * `attachmentId`, which the server takes too) and its name the title. Its
 * size and type are in its fields when this app made it (`createAttachment`);
 * a block another client made may have neither: they are 0 and empty, and
 * the screen leaves an unknown size out.
 */
export function attachmentOf(block: Schemas['Block']): Attachment {
  const fields = block.fields;
  const file = fields.fileId ?? fields.attachmentId;
  return {
    ...blockOf(block),
    type: 'attachment',
    fileId: typeof file === 'string' ? file : '',
    name: block.title,
    size: typeof fields.size === 'number' ? fields.size : 0,
    contentType: typeof fields.contentType === 'string' ? fields.contentType : '',
  };
}

/**
 * A category. The contract lists the boards of a category with a `hidden`
 * flag each; the screens keep the shown and the hidden apart.
 */
export function categoryOf(category: Schemas['CategoryBoards']): Category {
  const metadata = category.boardMetadata;
  return {
    id: category.id,
    name: category.name,
    userId: category.userId,
    teamId: category.teamId,
    type: category.type,
    collapsed: category.collapsed,
    sortOrder: category.sortOrder,
    boardIds: metadata.map((entry) => entry.boardId),
    hiddenBoardIds: metadata.filter((entry) => entry.hidden).map((entry) => entry.boardId),
    createAt: category.createAt,
    updateAt: category.updateAt,
  };
}

/** The settings, under the names of the contract. */
export function settingsOf(settings: Schemas['UserSettings']): AccountSettings {
  return {
    ...readRegionalSettings(settings),
    ...readDisplaySettings(settings),
    ...readLanguageSettings(settings),
  };
}

export function settingsChangeOf(patch: AccountSettingsPatch): Schemas['SettingsChange'] {
  return patch;
}

/** A role as the server keeps it: exactly one of the four flags. */
export function rolesOf(role: BoardRole): Schemas['MemberRoles'] {
  return {
    schemeAdmin: role === 'admin',
    schemeEditor: role === 'editor',
    schemeCommenter: role === 'commenter',
    schemeViewer: role === 'viewer',
  };
}

/** A public link; one that was never on has no token. */
export function sharingOf(sharing: Schemas['Sharing']): Sharing {
  return { enabled: sharing.enabled, token: sharing.token ?? '' };
}

/** A board and its first view, to be made in one request (`createBoardsAndBlocks`). */
export function newBoardAndViewOf(
  teamId: string,
  draft: BoardDraft,
  firstView: ViewDraft,
): Schemas['NewBoardsAndBlocks'] {
  const boardRef = 'new-board';
  return {
    boards: [
      {
        id: boardRef,
        teamId,
        type: draft.type ?? 'P',
        title: draft.title,
        description: draft.description ?? '',
        icon: draft.icon ?? '',
        cardProperties: (draft.cardProperties ?? []) as unknown as Record<string, unknown>[],
      },
    ],
    blocks: [
      {
        id: 'new-view',
        boardId: boardRef,
        type: 'view',
        title: firstView.title,
        fields: firstView.fields,
        createAt: 1,
        updateAt: 1,
      },
    ],
  };
}

/**
 * A change of a board, as `patchBoard` takes it. A minimum role the
 * contract does not have (none, or admin) is refused here as the server
 * would refuse it.
 */
export function boardPatchOf(patch: BoardPatch): Schemas['BoardPatch'] {
  const { minimumRole, updatedCardProperties, ...rest } = patch;
  if (minimumRole === '' || minimumRole === 'admin') {
    throw new BackendError(400, {
      detail: 'no such minimum role',
      errors: [refusedMember('minimumRole', 'no such minimum role')],
    });
  }
  return {
    ...rest,
    ...(minimumRole === undefined ? {} : { minimumRole }),
    ...(updatedCardProperties === undefined
      ? {}
      : { updatedCardProperties: updatedCardProperties as unknown as Record<string, unknown>[] }),
  };
}
