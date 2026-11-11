/**
 * The server's shapes (`contract.ts`, made from `contracts/openapi.json`)
 * and the shapes the screens work with (`lib/state/backend.ts`,
 * `lib/logic/wire.ts`), one into the other.
 *
 * Where the two differ the screens' shape stays and the difference is
 * settled here; each place is listed in `docs/contract-assumptions.md`.
 */
import { countTasks, type DocNode } from '../editor/schema.ts';
import { readDisplaySettings } from '../logic/display.ts';
import { readRegionalSettings } from '../logic/regional.ts';
import {
  createBoardView,
  createCard,
  type Block,
  type BlockType,
  type Board,
  type BoardMember,
  type BoardView,
  type PropertyTemplate,
} from '../logic/wire.ts';
import type {
  AccountSettings,
  AccountSettingsPatch,
  Attachment,
  CardDetail,
  CardDraft,
  Comment,
  CardSummary,
  Category,
  Team,
  User,
} from '../state/backend.ts';

import type { components } from './contract.ts';

export type Schemas = components['schemas'];

/**
 * A user. The contract's user has a name to log in with and nothing else:
 * no nickname, first or last name, and no email address (it is never
 * answered). The screens show the name to log in with.
 */
export function userOf(user: Schemas['User']): User {
  return {
    id: user.id,
    username: user.username,
    nickname: '',
    firstname: '',
    lastname: '',
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
 */
export function cardOfBlock(block: Schemas['Block']): CardSummary {
  const body = bodyOf(block.body);
  const tasks = body ? countTasks(body) : { total: 0, checked: 0 };
  return {
    ...createCard(blockOf(block)),
    hasBody: body !== null,
    tasksTotal: tasks.total,
    tasksDone: tasks.checked,
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
  };
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
 * A comment as the server keeps it today: a block whose title is its text.
 * The screens' comment is a document (`contract-assumptions.md`, B1); until
 * the server keeps one, each line of the text is a paragraph of plain text.
 */
export function commentOf(block: Schemas['Block']): Comment {
  const lines = block.title.trim().split(/\r?\n/);
  return {
    ...blockOf(block),
    type: 'comment',
    body:
      lines.join('') === ''
        ? null
        : {
            type: 'doc',
            content: lines.map((text) =>
              text
                ? { type: 'paragraph', content: [{ type: 'text', text }] }
                : { type: 'paragraph' },
            ),
          },
  };
}

/**
 * An attachment block. Its file is `fields.fileId` (older blocks name it
 * `attachmentId`, which the server takes too) and its name the title. The
 * block carries no size or type (`contract-assumptions.md`, B2): they are 0
 * and empty, and the screen leaves an unknown size out.
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
  return { ...readRegionalSettings(settings), ...readDisplaySettings(settings) };
}

export function settingsChangeOf(patch: AccountSettingsPatch): Schemas['SettingsChange'] {
  return patch;
}
