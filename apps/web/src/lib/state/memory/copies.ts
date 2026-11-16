/**
 * What a copy of a board or a wiki takes (`duplicateContainer`), as the server
 * copies: its blocks but the comments, with what is typed in them now, and
 * the files they show given copies of their own on the copy.
 */
import type { DocNode } from '../../editor/schema.ts';

import { flushContainers } from './bodies.ts';
import type { StoredBlock, World } from './world.ts';

/** The blocks a copy of the board or wiki takes: all but its comments. */
export function copiedBlocks(world: World, container: { id: string }): StoredBlock[] {
  flushContainers(world, [container]);
  return world.blocksOf(container.id).filter((block) => block.type !== 'comment');
}

/** The body with the files its images show renamed, where `rename` has another name. */
function renameImages(node: DocNode, rename: (id: string) => string): DocNode {
  const attrs =
    node.type === 'image' && typeof node.attrs?.fileId === 'string'
      ? { attrs: { ...node.attrs, fileId: rename(node.attrs.fileId) } }
      : {};
  const content = node.content
    ? { content: node.content.map((each) => renameImages(each, rename)) }
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
      world.files.set(made, { ...file, fileId: made, containerId: copyId });
    }
    return made;
  };
  for (const block of world.blocksOf(copyId)) {
    if (block.body) block.body = renameImages(block.body, renamed);
    if (block.file) block.file = { ...block.file, fileId: renamed(block.file.fileId) };
  }
}
