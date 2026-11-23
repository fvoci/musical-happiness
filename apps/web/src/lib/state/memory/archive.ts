/**
 * An archive of boards as the backend in memory writes it: a line of its
 * own version, then each board and its blocks, one per line. What the rooms
 * of their cards took is written into the cards first (`flushContainers`).
 */
import { createBoard, type Container } from '../../logic/wire.ts';
import type {
  DownloadedFile,
  DownloadOptions,
  ImportCounts,
  ImportFormat,
  ImportSummary,
  ImportWarning,
} from '../backend.ts';

import { FILE_BLOCK_IN_COMMENT, flushContainers } from './bodies.ts';
import { fail } from './refuse.ts';
import { hasFileBlock, MEMORY_ARCHIVE } from './shapes.ts';
import { oldestFirst, type StoredBlock, type World } from './world.ts';

export function archiveOf(
  world: World,
  containers: readonly Container[],
  options: DownloadOptions | undefined,
): DownloadedFile {
  flushContainers(world, containers);
  const lines = [JSON.stringify({ version: MEMORY_ARCHIVE })];
  for (const board of containers) {
    lines.push(JSON.stringify({ type: 'board', data: board }));
    // As the server writes them, which is the order an import numbers the cards in.
    for (const block of world.blocksOf(board.id).sort(oldestFirst)) {
      lines.push(JSON.stringify({ type: 'block', data: renamed(block, 'containerId', 'boardId') }));
    }
  }
  const bytes = new Blob([`${lines.join('\n')}\n`], { type: 'application/octet-stream' });
  options?.onProgress?.({ sentBytes: bytes.size, totalBytes: bytes.size });
  const day = new Date(world.now()).toISOString().slice(0, 10);
  return { bytes, name: `archive-${day}.boardarchive` };
}

/** The lines of an archive of the memory backend; 400 for what is none (`exportContainer`). */
export async function archiveLines(
  file: File,
): Promise<{ type?: string; version?: string; data?: unknown }[]> {
  let lines: { type?: string; version?: string; data?: unknown }[];
  try {
    lines = (await file.text())
      .split('\n')
      .filter((line) => line.trim() !== '')
      .map((line) => JSON.parse(line) as { type?: string; version?: string; data?: unknown });
  } catch {
    fail(400, 'not an archive');
  }
  if (lines[0]?.version !== MEMORY_ARCHIVE) fail(400, 'not an archive of this app');
  return lines;
}

/**
 * A block of an archive's line as the backend keeps it: the line names its
 * board `boardId`, the archive's own name, as it always has. A comment with a
 * file block refuses the archive (400), as the server refuses it whole.
 */
export function blockOfLine(data: unknown): StoredBlock {
  const block = (
    typeof data === 'object' && data !== null ? renamed(data, 'boardId', 'containerId') : data
  ) as StoredBlock;
  // As the server refuses the archive whole: a comment has no file block.
  if (block.type === 'comment' && block.body && hasFileBlock(block.body)) {
    fail(400, FILE_BLOCK_IN_COMMENT, { field: 'body' });
  }
  return block;
}

/** `value` with its member `from` named `to`, in the same place among the others. */
function renamed(value: object, from: string, to: string): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(value).map(([key, each]) => [key === from ? to : key, each]),
  );
}

/** The containers of `workspaceId` exported by `exportWorkspace` for `userId`. */
export function exportableContainers(
  world: World,
  workspaceId: string,
  userId: string,
): Container[] {
  return [...world.containers.values()].filter(
    (board) =>
      board.workspaceId === workspaceId &&
      !board.isTemplate &&
      (board.type === 'O' ||
        (world.members.get(board.id) ?? []).some((member) => member.userId === userId)),
  );
}

/** Reads an archive file into its containers and blocks (`importArchive`). */
export async function readArchive(
  file: File,
): Promise<{ containers: Container[]; blocks: StoredBlock[] }> {
  const lines = await archiveLines(file);
  return {
    containers: lines.filter((line) => line.type === 'board').map((line) => line.data as Container),
    blocks: lines.filter((line) => line.type === 'block').map((line) => blockOfLine(line.data)),
  };
}

interface TrelloCardLike {
  id?: unknown;
  name?: unknown;
  idList?: unknown;
  closed?: unknown;
}

interface TrelloListLike {
  id?: unknown;
  name?: unknown;
  closed?: unknown;
}

/** Reads an import file in `format` (`fvoci`, `trello`, or `notion`) for the memory backend. */
export async function readImportFile(
  file: File,
  format: ImportFormat,
  workspaceId: string,
): Promise<{ containers: Container[]; blocks: StoredBlock[]; skipped: ImportWarning[] }> {
  if (format === 'fvoci') {
    const parsed = await readArchive(file);
    return { ...parsed, skipped: [] };
  }
  if (file.size === 0) fail(400, 'cannot import an empty file');
  if (format === 'trello') return readTrelloImport(file, workspaceId);
  const head = new Uint8Array(await file.slice(0, 2).arrayBuffer());
  if (head[0] !== 0x50 || head[1] !== 0x4b) fail(400, 'invalid zip archive');
  const title = file.name.replace(/\.zip$/i, '') || 'Notion Import';
  const wiki: Container = { ...createBoard(), workspaceId, title, kind: 'wiki' };
  return { containers: [wiki], blocks: [], skipped: [] };
}

async function readTrelloImport(
  file: File,
  workspaceId: string,
): Promise<{ containers: Container[]; blocks: StoredBlock[]; skipped: ImportWarning[] }> {
  let raw: unknown;
  try {
    raw = JSON.parse(await file.text());
  } catch {
    fail(400, 'invalid Trello board JSON');
  }
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    fail(400, 'invalid Trello board JSON');
  }
  const data = raw as Record<string, unknown>;
  const hasLists = data.lists !== undefined;
  const hasCards = data.cards !== undefined;
  if (
    (!hasLists && !hasCards) ||
    (hasLists && !Array.isArray(data.lists)) ||
    (hasCards && !Array.isArray(data.cards))
  ) {
    fail(400, 'JSON does not look like a Trello board export');
  }
  const lists = (hasLists ? data.lists : []) as unknown[];
  const cards = (hasCards ? data.cards : []) as unknown[];
  const skipped: ImportWarning[] = [];
  const closedLists = new Set<string>();
  for (const item of lists) {
    if (typeof item !== 'object' || item === null) continue;
    const list = item as TrelloListLike;
    const listId = typeof list.id === 'string' ? list.id : '';
    const listName = typeof list.name === 'string' ? list.name : listId;
    if (list.closed === true && listId) {
      closedLists.add(listId);
      skipped.push({
        code: 'closed_item_skipped',
        item: listName,
        reason: 'archived list skipped',
      });
    }
  }
  const boardName = typeof data.name === 'string' ? data.name.trim() : '';
  const boardDesc = typeof data.desc === 'string' ? data.desc : '';
  const board: Container = {
    ...createBoard(),
    workspaceId,
    title: boardName || 'Trello Board',
    description: boardDesc,
  };
  const blocks: StoredBlock[] = [];
  for (const item of cards) {
    if (typeof item !== 'object' || item === null) continue;
    const card = item as TrelloCardLike;
    const cardId = typeof card.id === 'string' ? card.id : `trello-${String(blocks.length)}`;
    const cardName = typeof card.name === 'string' ? card.name : '';
    const cardListId = typeof card.idList === 'string' ? card.idList : '';
    if (card.closed === true || (cardListId && closedLists.has(cardListId))) {
      skipped.push({
        code: 'closed_item_skipped',
        item: cardName || cardId,
        reason: 'archived card skipped',
      });
      continue;
    }
    blocks.push({
      id: cardId,
      parentId: '',
      containerId: board.id,
      type: 'card',
      title: cardName,
      fields: {},
      body: null,
      createdBy: '',
      modifiedBy: '',
      schema: 1,
      createAt: 0,
      updateAt: 0,
      deleteAt: 0,
    });
  }
  return { containers: [board], blocks, skipped };
}

/** Builds an `ImportSummary` from containers and blocks inserted into the memory world. */
export function summarizeImport(
  format: ImportFormat,
  created: readonly Container[],
  blocks: readonly StoredBlock[],
  skipped: readonly ImportWarning[],
): ImportSummary {
  const counts: { -readonly [K in keyof ImportCounts]: number } = {
    boards: 0,
    wikis: 0,
    cards: 0,
    pages: 0,
    comments: 0,
    files: 0,
  };
  for (const container of created) {
    const kind = container.kind ?? 'board';
    if (kind === 'wiki') counts.wikis += 1;
    else if (kind === 'board') counts.boards += 1;
  }
  for (const block of blocks) {
    if (block.type === 'card') counts.cards += 1;
    else if (block.type === 'page') counts.pages += 1;
    else if (block.type === 'comment') counts.comments += 1;
  }
  return {
    format,
    imported: counts,
    membersLeftOut: 0,
    containerIds: created.map((container) => container.id),
    skipped,
    warnings: [],
  };
}
