/**
 * An archive of boards as the backend in memory writes it: a line of its
 * own version, then each board and its blocks, one per line. What the rooms
 * of their cards took is written into the cards first (`flushContainers`).
 */
import type { Container } from '../../logic/wire.ts';
import type { DownloadedFile, DownloadOptions } from '../backend.ts';

import { flushContainers } from './bodies.ts';
import { fail } from './refuse.ts';
import { MEMORY_ARCHIVE } from './shapes.ts';
import type { StoredBlock, World } from './world.ts';

export function archiveOf(
  world: World,
  containers: readonly Container[],
  options: DownloadOptions | undefined,
): DownloadedFile {
  flushContainers(world, containers);
  const lines = [JSON.stringify({ version: MEMORY_ARCHIVE })];
  for (const board of containers) {
    lines.push(JSON.stringify({ type: 'board', data: board }));
    for (const block of world.blocksOf(board.id)) {
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
 * board `boardId`, the archive's own name, as it always has.
 */
export function blockOfLine(data: unknown): StoredBlock {
  return renamed(data as object, 'boardId', 'containerId') as unknown as StoredBlock;
}

/** `value` with its member `from` named `to`, in the same place among the others. */
function renamed(value: object, from: string, to: string): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(value).map(([key, each]) => [key === from ? to : key, each]),
  );
}
