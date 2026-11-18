/**
 * What the memory backend's methods share: the server design's limits, the
 * refusal as the server makes it, the shapes its stored blocks are answered
 * in, and an order of lists nobody should lean on.
 */
import { cardPreviewOf } from '../../editor/card-preview.ts';
import { countTasks, MAX_BODY_BYTES, type DocNode } from '../../editor/schema.ts';
import {
  createBoardView,
  createCard,
  type Container,
  type ContainerMember,
  type BoardView,
} from '../../logic/wire.ts';
import {
  type Attachment,
  type CardDetail,
  type CardSummary,
  type Change,
  type Comment,
  type WikiPage,
} from '../backend.ts';

import { withPropertyOrder } from '../../logic/property-order.ts';
import { fail } from './refuse.ts';
import type { StoredBlock, StoredCard } from './world.ts';

/** The limits of the server design: section 5.7 (bodies) and 4.2, 5.6 (files). */
export const LIMITS = {
  bodyBytes: MAX_BODY_BYTES,
  fileBytes: 100 * 1024 * 1024,
  uploadPartBytes: 8 * 1024 * 1024,
  screenStateKeys: 100,
  screenStateValueBytes: 4 * 1024,
} as const;

export const SCREEN_STATE_KEY = /^[A-Za-z0-9_.-]{1,64}$/;

export const copy = <T>(value: T): T => structuredClone(value);

/** The first line of an archive of the memory backend. */
export const MEMORY_ARCHIVE = 'fvoci-memory-1';

/**
 * The board with its properties in `propertyIds`, which names every one of
 * them once, as the server takes it; 400 and nothing changes otherwise.
 */
export function inPropertyOrder(stored: Container, propertyIds: readonly string[]): Container {
  const ids = stored.cardProperties.map((template) => template.id);
  if (new Set(propertyIds).size !== propertyIds.length) {
    fail(400, 'the order names each property of the board once');
  }
  if (propertyIds.some((id) => !ids.includes(id)) || propertyIds.length !== ids.length) {
    fail(400, 'the order names every property of the board');
  }
  return withPropertyOrder(stored, propertyIds);
}

export function summaryOf(card: StoredCard): CardSummary {
  const { body, file: _file, ...block } = card;
  const tasks = body ? countTasks(body) : { total: 0, checked: 0 };
  return copy({
    ...createCard(block),
    hasBody: body !== null && body !== undefined,
    tasksTotal: tasks.total,
    tasksDone: tasks.checked,
  });
}

export function detailOf(card: StoredCard): CardDetail {
  return { ...summaryOf(card), body: copy(card.body ?? null) };
}

export function viewOf(view: StoredBlock): BoardView {
  const { body: _body, file: _file, ...block } = view;
  return copy(createBoardView(block));
}

export function commentOf(block: StoredBlock): Comment {
  const { body, file: _file, ...rest } = block;
  return copy({ ...rest, type: 'comment', body: body ?? null });
}

export function attachmentOf(block: StoredBlock): Attachment {
  const { body: _body, file, ...rest } = block;
  if (!file) throw new Error(`attachment ${block.id} has no file`);
  return copy({ ...rest, type: 'attachment', ...file });
}

/** The place of a page among its siblings, as its `fields` have it. */
export function orderOf(block: StoredBlock): string {
  const order = (block.fields as { order?: unknown }).order;
  return typeof order === 'string' ? order : '';
}

/** A page as its tree lists it. */
export function pageOf(block: StoredBlock): WikiPage {
  const icon = (block.fields as { icon?: unknown }).icon;
  return {
    id: block.id,
    wikiId: block.containerId,
    parentId: block.parentId,
    title: block.title,
    icon: typeof icon === 'string' ? icon : '',
    order: orderOf(block),
    createAt: block.createAt,
    updateAt: block.updateAt,
  };
}

/** What the listeners of a board or a wiki are told of a block made or changed: a page, or another block. */
export function changeOf(containerId: string, block: StoredBlock): Change {
  if (block.type === 'page') return { kind: 'pages', containerId, pages: [pageOf(block)] };
  return { kind: 'blocks', containerId, blocks: [wireOf(block)] };
}

export function wireOf(block: StoredBlock): BoardView | CardSummary | Comment | Attachment {
  switch (block.type) {
    case 'view':
      return viewOf(block);
    case 'card':
      // A change message carries the body, which the preview is made from (as on the server).
      return { ...summaryOf(block as StoredCard), preview: cardPreviewOf(block.body) };
    case 'comment':
      return commentOf(block);
    default:
      return attachmentOf(block);
  }
}

/** A list in an order nobody should rely on: by a hash of the identifiers. */
export function scramble<T extends { id: string } | { userId: string }>(items: readonly T[]): T[] {
  const key = (item: T): number => {
    const text = 'id' in item ? item.id : item.userId;
    let hash = 0;
    for (let at = 0; at < text.length; at += 1) hash = (hash * 31 + text.charCodeAt(at)) % 9973;
    return hash;
  };
  return [...items].sort((first, second) => key(first) - key(second));
}

/** Whether a node names a file: an image, or a file block. */
export const namesFile = (node: DocNode): boolean =>
  (node.type === 'image' || node.type === 'file') && typeof node.attrs?.fileId === 'string';

/** The files a body names, by its images and file blocks. */
export function bodyFileIds(node: DocNode, found: string[] = []): string[] {
  if (namesFile(node)) found.push(String(node.attrs?.fileId));
  for (const child of node.content ?? []) bodyFileIds(child, found);
  return found;
}

/** Whether a body has a file block, which a comment's may not. */
export function hasFileBlock(node: DocNode): boolean {
  return node.type === 'file' || (node.content ?? []).some(hasFileBlock);
}

export function memberOf(
  containerId: string,
  userId: string,
  role: 'admin' | 'editor' | 'commenter' | 'viewer',
): ContainerMember {
  return {
    containerId,
    userId,
    minimumRole: 'editor',
    schemeAdmin: role === 'admin',
    schemeEditor: role === 'editor',
    schemeCommenter: role === 'commenter',
    schemeViewer: role === 'viewer',
    synthetic: false,
  };
}
