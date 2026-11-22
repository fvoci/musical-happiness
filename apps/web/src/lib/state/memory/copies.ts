/**
 * What a copy of a board or a wiki takes (`duplicateContainer`), as the server
 * copies: its blocks but the comments, with what is typed in them now, and
 * the files they show given copies of their own on the copy; and how a copy,
 * or the board of an archive, is stored.
 */
import type { DocNode } from '../../editor/schema.ts';
import type { Container } from '../../logic/wire.ts';

import { flushContainers } from './bodies.ts';
import { numberBlock } from './card-numbers.ts';
import { fileAsNew } from './categories.ts';
import { renameCopied } from './references.ts';
import { copy, memberOf, namesFile } from './shapes.ts';
import type { StoredBlock, World } from './world.ts';

/** The blocks a copy of the board or wiki takes: all but its comments. */
export function copiedBlocks(world: World, container: { id: string }): StoredBlock[] {
  flushContainers(world, [container]);
  return world.blocksOf(container.id).filter((block) => block.type !== 'comment');
}

/** The language of a tag among the two the server writes its own words in. */
function writtenIn(tag: string): 'en' | 'ko' | undefined {
  const language = tag.split('-')[0]?.toLowerCase();
  return language === 'ko' || language === 'en' ? language : undefined;
}

/**
 * The title of a copy, as the server titles it: a copy of the same kind is
 * the original's with "copy" after it, a template made from a board "New
 * board template", and a board made from a template keeps the title. The
 * words are English or Korean: by the account's `language`; where that is
 * left to the device or is another language, by the first of the two the
 * browser asks for (its `Accept-Language`); failing both, English.
 */
export function copyTitle(
  source: { title: string; isTemplate: boolean },
  asTemplate: boolean,
  language: string,
): string {
  if (source.isTemplate !== asTemplate && !asTemplate) return source.title;
  const asked = typeof navigator === 'undefined' ? [] : navigator.languages;
  const korean = (writtenIn(language) ?? asked.map(writtenIn).find(Boolean)) === 'ko';
  if (source.isTemplate !== asTemplate) return korean ? '새 보드 템플릿' : 'New board template';
  return `${source.title} ${korean ? '사본' : 'copy'}`;
}

/** The body with the files its images and file blocks name renamed, where `rename` has another name. */
function renameFiles(node: DocNode, rename: (id: string) => string): DocNode {
  const attrs = namesFile(node)
    ? { attrs: { ...node.attrs, fileId: rename(String(node.attrs?.fileId)) } }
    : {};
  const content = node.content
    ? { content: node.content.map((each) => renameFiles(each, rename)) }
    : {};
  return { ...node, ...attrs, ...content };
}

/**
 * Gives the blocks of the copy `copyId` copies of their own of the files of
 * `sourceId` they show or attach, as the server does: a body of the copy
 * that showed the source's files could not be written again.
 */
export function giveCopiesFiles(world: World, sourceId: string, copyId: string): void {
  const copies = new Map<string, string>();
  const renamed = (fileId: string): string => {
    const file = world.files.get(fileId);
    if (file?.containerId !== sourceId) return fileId;
    let made = copies.get(fileId);
    if (made === undefined) {
      made = world.newId();
      copies.set(fileId, made);
      // The newest version alone, by nobody, as the server copies a file.
      const copied = { ...file, fileId: made, containerId: copyId, createdBy: '' };
      delete copied.versions;
      world.files.set(made, copied);
    }
    return made;
  };
  for (const block of world.blocksOf(copyId)) {
    if (block.body) block.body = renameFiles(block.body, renamed);
    if (block.file) block.file = { ...block.file, fileId: renamed(block.file.fileId) };
  }
}

/** What is copied: a board and its blocks, into a workspace. */
export interface Copied {
  source: Container;
  blocks: readonly StoredBlock[];
  workspaceId: string;
}

/**
 * Stores a board and its blocks under new identifiers, `userId` its admin,
 * and answers the board. As on the server, only the references it renames
 * follow: a block's board and parent, and what `renameCopied` renames. Its
 * cards are new cards: they are numbered from 1, in the order they come.
 */
export function insertCopy(world: World, userId: string, copied: Copied): Container {
  const { source, blocks, workspaceId } = copied;
  const now = world.now();
  const ids = new Map<string, string>([[source.id, world.newId()]]);
  for (const block of blocks) ids.set(block.id, world.newId());
  const renamed = (id: string): string => ids.get(id) ?? id;
  const cardIds = new Set(blocks.filter((block) => block.type === 'card').map(({ id }) => id));
  const board: Container = {
    ...copy(source),
    id: renamed(source.id),
    workspaceId,
    createdBy: userId,
    modifiedBy: userId,
    type: 'P',
    isBuiltin: false,
    createAt: now,
    updateAt: now,
  };
  world.containers.set(board.id, board);
  world.members.set(board.id, [memberOf(board.id, userId, 'admin')]);
  fileAsNew(world, userId, board);
  for (const block of blocks) {
    const made = copy(block);
    renameCopied(source, made, ids, cardIds);
    const stored: StoredBlock = {
      ...made,
      id: renamed(block.id),
      containerId: board.id,
      parentId: renamed(made.parentId),
      createAt: now,
      updateAt: now,
    };
    numberBlock(world, stored);
    world.blocks.set(stored.id, stored);
  }
  giveCopiesFiles(world, source.id, board.id);
  return board;
}
