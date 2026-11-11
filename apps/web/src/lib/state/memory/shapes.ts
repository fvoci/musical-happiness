/**
 * What the memory backend's methods share: the server design's limits, the
 * refusal as the server makes it, the shapes its stored blocks are answered
 * in, and an order of lists nobody should lean on.
 */
import { cardPreviewOf } from '../../editor/card-preview.ts';
import { countTasks, MAX_BODY_BYTES, type DocNode } from '../../editor/schema.ts';
import { createBoardView, createCard, type BoardMember, type BoardView } from '../../logic/wire.ts';
import {
  BackendError,
  problemOf,
  refusedMember,
  type Attachment,
  type CardDetail,
  type CardSummary,
  type Comment,
  type ErrorStatus,
} from '../backend.ts';

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
 * Rejects as the server does: with the status and the detail, and, where the
 * refusal is about a member of the request, that member in `errors` (with
 * the reason); a reason about no member is the problem's own (`last_admin`).
 */
export function fail(
  status: ErrorStatus,
  detail: string,
  extra: { field?: string; within?: string; reason?: string; limit?: number } = {},
): never {
  const { field, within, reason, limit } = extra;
  const entry = field === undefined ? undefined : refusedMember(field, detail, reason);
  // A member of a map the request carries is under the map's own member.
  if (entry && within !== undefined) entry.pointer = `/${within}${entry.pointer ?? ''}`;
  throw new BackendError(status, {
    ...problemOf(status, detail),
    ...(limit === undefined ? {} : { limit }),
    ...(entry === undefined ? {} : { errors: [entry] }),
    // A reason of the problem names its type, as on the server (`last_admin`: `/problems/last-admin`).
    ...(entry === undefined && reason !== undefined
      ? { reason, type: `/problems/${reason.replaceAll('_', '-')}` }
      : {}),
  });
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

/** The files a body shows. */
export function imageFileIds(node: DocNode, found: string[] = []): string[] {
  if (node.type === 'image' && typeof node.attrs?.fileId === 'string')
    found.push(node.attrs.fileId);
  for (const child of node.content ?? []) imageFileIds(child, found);
  return found;
}

export function memberOf(
  boardId: string,
  userId: string,
  role: 'admin' | 'editor' | 'commenter' | 'viewer',
): BoardMember {
  return {
    boardId,
    userId,
    minimumRole: 'editor',
    schemeAdmin: role === 'admin',
    schemeEditor: role === 'editor',
    schemeCommenter: role === 'commenter',
    schemeViewer: role === 'viewer',
    synthetic: false,
  };
}
