/**
 * What a copy of a board or a wiki takes (`duplicateContainer`), as the server
 * copies: its blocks but the comments, with what is typed in them now, and
 * the files they show given copies of their own on the copy.
 */
import type { DocNode } from '../../editor/schema.ts';

import { flushContainers } from './bodies.ts';
import { namesFile } from './shapes.ts';
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
