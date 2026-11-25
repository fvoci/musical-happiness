/**
 * The data the in-memory backend starts with: the fvoci built-in templates of
 * `./seed/`, seeded both as live workspace boards (English) and as built-in
 * templates (English and Korean). The tests are written against them.
 *
 * What a template file does not carry is added here for the live boards so
 * that the screens have something to show: six people, who made and was last
 * on each card, the people a person property points at, a few comments, one
 * attachment, a sidebar, and the numbers of the cards (`card-numbers.ts`).
 *
 * The result is the same on every run: identifiers and times come from a
 * counter, not from the clock.
 */
import { DEFAULT_ACCOUNT_SETTINGS } from '../../logic/account-settings.ts';
import { createBoard, type Container } from '../../logic/wire.ts';
import type { Category, User } from '../backend.ts';

import { seedUsers } from './users.ts';
import { memberOf } from './backend.ts';
import { numberedWorld } from './card-numbers.ts';
import { seedCourse } from './seed-course.ts';
import {
  boardCardProperties,
  placeSeedFiles,
  seedBoardBlocks,
  seedBuiltinTemplate,
  type SeedTemplate,
} from './seed-templates.ts';
import { World } from './world.ts';

export type {
  LegacySeedFile,
  SeedAsset,
  SeedCourseChapter,
  SeedCourseSchedule,
  SeedCourseUnit,
  SeedFile,
  SeedTemplate,
} from './seed-templates.ts';

type RawSeedTemplate = Omit<SeedTemplate, 'board' | 'views' | 'cards' | 'files'> & {
  board: Omit<SeedTemplate['board'], 'cardProperties'> & {
    cardProperties?: SeedTemplate['board']['cardProperties'];
  };
  views?: SeedTemplate['views'];
  cards?: SeedTemplate['cards'];
  files?: SeedTemplate['files'];
};

const templateFiles = import.meta.glob<RawSeedTemplate>('./seed/[0-9]*.json', {
  import: 'default',
});

/** The boards and courses of `./seed/`, in the order of their file names. */
export async function loadTemplates(): Promise<SeedTemplate[]> {
  const names = Object.keys(templateFiles).sort();
  const loaded = await Promise.all(
    names.map((name) => {
      const load = templateFiles[name];
      if (!load) throw new Error(`no loader for ${name}`);
      return load();
    }),
  );
  return loaded.map((raw) => ({
    ...raw,
    board: {
      ...raw.board,
      cardProperties: raw.board.cardProperties ?? [],
    },
    views: raw.views ?? [],
    cards: raw.cards ?? [],
    files: raw.files ?? [],
  }));
}

/** When the seeded world begins: 2026-10-01 00:00 UTC. Everything is made after it, a minute apart. */
export const SEED_EPOCH = Date.UTC(2026, 9, 1);
const MINUTE = 60_000;

const PEOPLE: readonly Omit<User, 'id' | 'createdAt' | 'updatedAt' | 'deletedAt'>[] = [
  {
    username: 'hana',
    displayName: '김하나',
    email: 'hana@example.com',
  },
  {
    username: 'minjun',
    displayName: '이민준',
    email: 'minjun@example.com',
  },
  {
    username: 'seoyeon',
    displayName: '박서연',
    email: 'seoyeon@example.com',
  },
  {
    username: 'alex',
    displayName: '',
    email: 'alex@example.com',
  },
  {
    username: 'meiling',
    displayName: '陳美玲',
    email: 'meiling@example.com',
  },
  {
    username: 'haruto',
    displayName: '佐藤陽翔',
    email: 'haruto@example.com',
  },
];

/** The sidebar of the reader: which templates are filed under which category. */
const SIDEBAR: readonly { name: string; type: Category['type']; keys: readonly string[] }[] = [
  {
    name: 'Projects',
    type: 'custom',
    keys: ['project-schedule', 'issue-tracker', 'meeting-notes'],
  },
  {
    name: 'Operations',
    type: 'custom',
    keys: ['content-calendar', 'customer-contacts'],
  },
  { name: 'Personal', type: 'custom', keys: ['study-notes'] },
  // Everything else lands here; an empty list of keys means "the rest".
  { name: 'Boards', type: 'system', keys: [] },
];

/** The board on which the reader may only look. It shows what a screen does without the right to edit. */
export const VIEW_ONLY_KEY = 'study-notes';

export interface SeededWorld {
  world: World;
  /** The user the app is signed in as. */
  readerId: string;
}

/** A world holding the given templates as live workspace boards and built-in templates. */
export function seedWorld(templates: readonly SeedTemplate[]): SeededWorld {
  let tick = 0;
  const clock = (): number => SEED_EPOCH + (tick += 1) * MINUTE;
  const world = new World(clock);

  world.workspace = { id: world.newId(), title: 'fvoci', kind: 'shared' };

  const users: User[] = PEOPLE.map((person, index) => {
    const at = world.now();
    return {
      ...person,
      id: world.newId(),
      isAdmin: index === 0,
      createdAt: at,
      updatedAt: at,
    };
  });
  seedUsers(world, users);
  const reader = users[0];
  if (!reader) throw new Error('the seed has no people');
  world.settings.set(reader.id, { ...DEFAULT_ACCOUNT_SETTINGS });

  const hasLocalized = templates.some((template) => template.language !== undefined);
  const liveTemplates = templates.filter(
    (template) => template.board.kind !== 'course' && (!hasLocalized || template.language === 'en'),
  );
  const builtinTemplates = templates.filter((template) => template.language !== undefined);

  const boardIdOf = new Map<string, string>();
  liveTemplates.forEach((template, templateIndex) => {
    const boardId = world.newId();
    boardIdOf.set(template.key, boardId);
    const at = world.now();
    const others = users.filter((user) => user !== reader);
    // Three or four people per board, a different set on each.
    const people = [reader, ...others.filter((_, index) => (index + templateIndex) % 5 < 3)];
    const board: Container = {
      ...createBoard(),
      id: boardId,
      workspaceId: world.workspace.id,
      createdBy: reader.id,
      modifiedBy: reader.id,
      visibility: 'private',
      minimumRole: 'editor',
      title: template.board.title,
      description: template.board.description,
      icon: template.board.icon,
      showDescription: template.board.showDescription,
      cardProperties: boardCardProperties(template.board.cardProperties),
      createdAt: at,
      updatedAt: at,
    };
    world.containers.set(boardId, board);
    world.members.set(
      boardId,
      people.map((user, index) =>
        memberOf(
          boardId,
          user.id,
          user === reader
            ? template.key === VIEW_ONLY_KEY
              ? 'viewer'
              : 'admin'
            : index % 3 === 0
              ? 'commenter'
              : 'editor',
        ),
      ),
    );
    const assetIdOf = placeSeedFiles(world, boardId, template.files);
    seedBoardBlocks(world, board, template, assetIdOf, {
      reader,
      people,
      live: true,
      templateIndex,
    });
  });

  // One attachment, on the first card of the first live board.
  seedAttachment(world, reader);

  const filed = new Set(SIDEBAR.flatMap((category) => category.keys));
  const categories: Category[] = SIDEBAR.map((category, index) => {
    const at = world.now();
    const keys =
      category.keys.length > 0
        ? category.keys
        : liveTemplates.map((template) => template.key).filter((key) => !filed.has(key));
    return {
      id: world.newId(),
      name: category.name,
      userId: reader.id,
      workspaceId: world.workspace.id,
      type: category.type,
      collapsed: false,
      sortOrder: index * 10,
      containerIds: keys.flatMap((key) => boardIdOf.get(key) ?? []),
      hiddenContainerIds: [],
      createdAt: at,
      updatedAt: at,
    };
  });

  const hasBuiltinCourse = builtinTemplates.some((template) => template.board.kind === 'course');
  if (liveTemplates.length > 1 || hasBuiltinCourse) {
    seedCourse(world, reader, users, categories, !hasBuiltinCourse);
  }
  world.categories.set(reader.id, categories);

  for (const template of builtinTemplates) seedBuiltinTemplate(world, template, reader);

  // From here on the world lives in real time.
  world.useClock(Date.now);
  return { world: numberedWorld(world), readerId: reader.id };
}

/** Seeds one sample attachment on the first non-template card of the first live board. */
function seedAttachment(world: World, reader: User): void {
  const firstCard = [...world.blocks.values()].find(
    (block) =>
      block.type === 'card' &&
      block.fields.isTemplate !== true &&
      world.containers.get(block.containerId)?.isTemplate !== true,
  );
  if (!firstCard) return;
  const notes = new Blob(['Notes from the kick-off.\n'], { type: 'text/plain' });
  const fileId = world.newId();
  world.files.set(fileId, {
    fileId,
    containerId: firstCard.containerId,
    name: 'kick-off-notes.txt',
    size: notes.size,
    contentType: 'text/plain',
    blob: notes,
  });
  const made = world.now();
  const id = world.newId();
  world.blocks.set(id, {
    id,
    containerId: firstCard.containerId,
    parentId: firstCard.id,
    createdBy: reader.id,
    modifiedBy: reader.id,
    type: 'attachment',
    title: 'kick-off-notes.txt',
    fields: {},
    createdAt: made,
    updatedAt: made,
    file: { fileId, name: 'kick-off-notes.txt', size: notes.size, contentType: 'text/plain' },
  });
}
