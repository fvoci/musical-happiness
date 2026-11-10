/**
 * The server's shapes (`contract.ts`, made from `contracts/openapi.json`)
 * and the shapes the screens work with (`lib/state/backend.ts`,
 * `lib/logic/wire.ts`), one into the other.
 *
 * Where the two differ the screens' shape stays and the difference is
 * settled here; each place is listed in `docs/contract-assumptions.md`.
 */
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
  CardDetail,
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

/** A block of the API as the screens' block. */
export function blockOf(block: Schemas['Block']): Block {
  return { ...block, type: block.type as BlockType, fields: { ...block.fields } };
}

export function viewOf(block: Schemas['Block']): BoardView {
  return createBoardView(blockOf(block));
}

/**
 * The card summary of a card block, as a change message brings it. The API
 * has no body yet (`docs/contract-pending.md`, 1), so a card has none and no
 * tasks.
 */
export function cardOfBlock(block: Schemas['Block']): CardSummary {
  return { ...createCard(blockOf(block)), hasBody: false, tasksTotal: 0, tasksDone: 0 };
}

/** A card of the cards API: its members are where a card block has its `fields`. */
export function cardOf(card: Schemas['Card']): CardSummary {
  return cardOfBlock({
    id: card.id,
    boardId: card.boardId,
    parentId: '',
    createdBy: card.createdBy,
    modifiedBy: card.modifiedBy,
    schema: 1,
    type: 'card',
    title: card.title,
    fields: {
      icon: card.icon,
      isTemplate: card.isTemplate,
      properties: card.properties,
      contentOrder: card.contentOrder,
    },
    createAt: card.createAt,
    updateAt: card.updateAt,
    deleteAt: card.deleteAt,
  });
}

export function cardDetailOf(card: Schemas['Card']): CardDetail {
  return { ...cardOf(card), body: null };
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

/**
 * The settings. The contract names two of the display settings otherwise
 * than the screens still do (`theme` is `brightness`, `haptics` is
 * `vibration`; `docs/contract-assumptions.md`, F1).
 */
export function settingsOf(settings: Schemas['UserSettings']): AccountSettings {
  return {
    ...readRegionalSettings(settings),
    ...readDisplaySettings({
      brightness: settings.theme,
      cardOpen: settings.cardOpen,
      density: settings.density,
      vibration: settings.haptics,
    }),
  };
}

export function settingsChangeOf(patch: AccountSettingsPatch): Schemas['SettingsChange'] {
  const { brightness, vibration, ...rest } = patch;
  const change: Schemas['SettingsChange'] = { ...rest };
  if (brightness !== undefined) change.theme = brightness;
  if (vibration !== undefined) change.haptics = vibration;
  return change;
}
