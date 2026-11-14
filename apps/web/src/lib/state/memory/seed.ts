/**
 * The data the in-memory backend starts with: the built-in templates, as
 * boards a small team is working on.
 *
 * The boards, views, cards and bodies are the built-in templates the server
 * embeds (`crates/server/assets/templates/`), read as they are. What a
 * template does not have is added here so that the screens have something to
 * show: six people, who made and was last on each card, the people a person
 * property points at, a few comments, one attachment, and a sidebar.
 *
 * The result is the same on every run: identifiers and times come from a
 * counter, not from the clock.
 */
import type { DocNode } from '../../editor/schema.ts';
import { DEFAULT_ACCOUNT_SETTINGS } from '../../logic/account-settings.ts';
import {
  createBoard,
  type Board,
  type PropertyTemplate,
  type PropertyType,
  type PropertyValue,
} from '../../logic/wire.ts';
import type { Category, User } from '../backend.ts';

import { seedUsers } from './users.ts';
import { memberOf } from './backend.ts';
import { World, type StoredBlock } from './world.ts';

/** One template file of `crates/server/assets/templates/`. */
export interface SeedTemplate {
  key: string;
  onboarding: boolean;
  board: {
    id: string;
    title: string;
    description: string;
    icon: string;
    showDescription: boolean;
    cardProperties: {
      id: string;
      name: string;
      type: string;
      options: PropertyTemplate['options'];
    }[];
  };
  views: { id: string; title: string; fields: Record<string, unknown> }[];
  cards: {
    id: string;
    title: string;
    fields: { icon: string; isTemplate: boolean; properties: Record<string, PropertyValue> };
    body: DocNode | null;
  }[];
  files: { fileId: string; archiveMember: string; bytes: number }[];
}

const templateFiles = import.meta.glob<SeedTemplate>(
  '../../../../../../crates/server/assets/templates/[0-9]*.json',
  { import: 'default' },
);

/** The built-in templates, in the order of their file names. */
export async function loadTemplates(): Promise<SeedTemplate[]> {
  const names = Object.keys(templateFiles).sort();
  return Promise.all(
    names.map((name) => {
      const load = templateFiles[name];
      if (!load) throw new Error(`no loader for ${name}`);
      return load();
    }),
  );
}

/** When the seeded world begins: 2026-10-01 00:00 UTC. Everything is made after it, a minute apart. */
export const SEED_EPOCH = Date.UTC(2026, 9, 1);
const MINUTE = 60_000;

const PEOPLE: readonly Omit<User, 'id' | 'createAt' | 'updateAt' | 'deleteAt'>[] = [
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
    name: 'Team',
    type: 'custom',
    keys: ['project-tasks', 'sprint-planner', 'roadmap', 'meeting-agenda', 'team-retrospective'],
  },
  {
    name: 'Research',
    type: 'custom',
    keys: ['user-research-sessions', 'competitive-analysis', 'content-calendar'],
  },
  { name: 'Personal', type: 'custom', keys: ['personal-tasks', 'personal-goals'] },
  // Everything else lands here; an empty list of keys means "the rest".
  { name: 'Boards', type: 'system', keys: [] },
];

/** The board on which the reader may only look. It shows what a screen does without the right to edit. */
const VIEW_ONLY_KEY = 'company-goals-okrs';

const KNOWN_TYPES: readonly PropertyType[] = [
  'text',
  'number',
  'select',
  'multiSelect',
  'date',
  'person',
  'multiPerson',
  'file',
  'checkbox',
  'url',
  'email',
  'phone',
  'createdTime',
  'createdBy',
  'updatedTime',
  'updatedBy',
];

const paragraph = (text: string): DocNode => ({
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
});

const COMMENTS = [
  'I can take this one.',
  '다음 주 화요일까지 초안을 올릴게요.',
  'Is this still blocked on the review?',
];

export interface SeededWorld {
  world: World;
  /** The user the app is signed in as. */
  readerId: string;
}

/** A world holding the given templates as boards. */
export function seedWorld(templates: readonly SeedTemplate[]): SeededWorld {
  let tick = 0;
  const clock = (): number => SEED_EPOCH + (tick += 1) * MINUTE;
  const world = new World(clock);

  world.workspace = { id: world.newId(), title: 'fvoci' };

  const users: User[] = PEOPLE.map((person, index) => {
    const at = world.now();
    return {
      ...person,
      id: world.newId(),
      isAdmin: index === 0,
      createAt: at,
      updateAt: at,
      deleteAt: 0,
    };
  });
  seedUsers(world, users);
  const reader = users[0];
  if (!reader) throw new Error('the seed has no people');
  world.settings.set(reader.id, { ...DEFAULT_ACCOUNT_SETTINGS });

  const boardIdOf = new Map<string, string>();
  templates.forEach((template, templateIndex) => {
    const boardId = world.newId();
    boardIdOf.set(template.key, boardId);
    const at = world.now();
    const others = users.filter((user) => user !== reader);
    // Three or four people per board, a different set on each.
    const people = [reader, ...others.filter((_, index) => (index + templateIndex) % 5 < 3)];
    const board: Board = {
      ...createBoard(),
      id: boardId,
      workspaceId: world.workspace.id,
      createdBy: reader.id,
      modifiedBy: reader.id,
      type: 'P',
      minimumRole: 'editor',
      title: template.board.title,
      description: template.board.description,
      icon: template.board.icon,
      showDescription: template.board.showDescription,
      cardProperties: template.board.cardProperties.map((property) => ({
        id: property.id,
        name: property.name,
        type: KNOWN_TYPES.includes(property.type as PropertyType)
          ? (property.type as PropertyType)
          : 'unknown',
        options: property.options.map((option) => ({ ...option })),
      })),
      createAt: at,
      updateAt: at,
    };
    world.boards.set(boardId, board);
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

    // The files the bodies name. Their bytes are not in this repository.
    for (const file of template.files) {
      world.files.set(file.fileId, {
        fileId: file.fileId,
        boardId,
        name: file.archiveMember.split('/').pop() ?? file.fileId,
        size: file.bytes,
        contentType: 'image/png',
      });
    }

    // Cards first: the views name them in their card order.
    const cardIdOf = new Map<string, string>();
    template.cards.forEach((card, cardIndex) => {
      const id = world.newId();
      cardIdOf.set(card.id, id);
      const made = world.now();
      const author = people[cardIndex % people.length] ?? reader;
      const properties: Record<string, PropertyValue> = structuredClone(card.fields.properties);
      // A person property of a template points at nobody we have: give it one of the people.
      board.cardProperties.forEach((property, propertyIndex) => {
        const person = people[(cardIndex + propertyIndex + 1) % people.length] ?? reader;
        if (property.type === 'person') properties[property.id] = person.id;
        if (property.type === 'multiPerson') properties[property.id] = [person.id, author.id];
      });
      const block: StoredBlock = {
        id,
        boardId,
        parentId: boardId,
        createdBy: author.id,
        modifiedBy: author.id,
        schema: 1,
        type: 'card',
        title: card.title,
        fields: { icon: card.fields.icon, isTemplate: card.fields.isTemplate, properties },
        createAt: made,
        updateAt: made,
        deleteAt: 0,
        body: card.body,
      };
      world.blocks.set(id, block);

      // Two comments on the first card of every board, one on the second.
      const comments = card.fields.isTemplate ? 0 : cardIndex === 0 ? 2 : cardIndex === 1 ? 1 : 0;
      for (let index = 0; index < comments; index += 1) {
        const commenter = people[(cardIndex + index + 1) % people.length] ?? reader;
        const written = world.now();
        const commentId = world.newId();
        world.blocks.set(commentId, {
          id: commentId,
          boardId,
          parentId: id,
          createdBy: commenter.id,
          modifiedBy: commenter.id,
          schema: 1,
          type: 'comment',
          title: '',
          fields: {},
          createAt: written,
          updateAt: written,
          deleteAt: 0,
          body: paragraph(COMMENTS[(templateIndex + index) % COMMENTS.length] ?? ''),
        });
      }
    });
    for (const view of template.views) {
      const made = world.now();
      const fields = structuredClone(view.fields);
      if (Array.isArray(fields.cardOrder)) {
        fields.cardOrder = (fields.cardOrder as string[]).map((id) => cardIdOf.get(id) ?? id);
      }
      if (typeof fields.defaultTemplateId === 'string' && fields.defaultTemplateId !== '') {
        fields.defaultTemplateId = cardIdOf.get(fields.defaultTemplateId) ?? '';
      }
      const id = world.newId();
      world.blocks.set(id, {
        id,
        boardId,
        parentId: boardId,
        createdBy: reader.id,
        modifiedBy: reader.id,
        schema: 1,
        type: 'view',
        title: view.title,
        fields,
        createAt: made,
        updateAt: made,
        deleteAt: 0,
      });
    }
  });

  // One attachment, on the first card of the first board.
  const firstCard = [...world.blocks.values()].find((block) => block.type === 'card');
  if (firstCard) {
    const notes = new Blob(['Notes from the kick-off.\n'], { type: 'text/plain' });
    const fileId = world.newId();
    world.files.set(fileId, {
      fileId,
      boardId: firstCard.boardId,
      name: 'kick-off-notes.txt',
      size: notes.size,
      contentType: 'text/plain',
      blob: notes,
    });
    const made = world.now();
    const id = world.newId();
    world.blocks.set(id, {
      id,
      boardId: firstCard.boardId,
      parentId: firstCard.id,
      createdBy: reader.id,
      modifiedBy: reader.id,
      schema: 1,
      type: 'attachment',
      title: 'kick-off-notes.txt',
      fields: {},
      createAt: made,
      updateAt: made,
      deleteAt: 0,
      file: { fileId, name: 'kick-off-notes.txt', size: notes.size, contentType: 'text/plain' },
    });
  }

  const filed = new Set(SIDEBAR.flatMap((category) => category.keys));
  const categories: Category[] = SIDEBAR.map((category, index) => {
    const at = world.now();
    const keys =
      category.keys.length > 0
        ? category.keys
        : templates.map((template) => template.key).filter((key) => !filed.has(key));
    return {
      id: world.newId(),
      name: category.name,
      userId: reader.id,
      workspaceId: world.workspace.id,
      type: category.type,
      collapsed: false,
      sortOrder: index * 10,
      boardIds: keys.flatMap((key) => boardIdOf.get(key) ?? []),
      hiddenBoardIds: [],
      createAt: at,
      updateAt: at,
    };
  });
  world.categories.set(reader.id, categories);

  // From here on the world lives in real time.
  world.useClock(Date.now);
  return { world, readerId: reader.id };
}
