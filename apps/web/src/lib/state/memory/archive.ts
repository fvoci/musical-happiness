/**
 * An archive of boards as the backend in memory writes it: a line of its
 * own version, then each board and its blocks, one per line. What the rooms
 * of their cards took is written into the cards first (`flushBoards`).
 */
import type { Container } from '../../logic/wire.ts';
import type { DownloadedFile, DownloadOptions } from '../backend.ts';

import { flushBoards } from './bodies.ts';
import { fail } from './refuse.ts';
import { MEMORY_ARCHIVE } from './shapes.ts';
import type { World } from './world.ts';

export function archiveOf(
  world: World,
  boards: readonly Container[],
  options: DownloadOptions | undefined,
): DownloadedFile {
  flushBoards(world, boards);
  const lines = [JSON.stringify({ version: MEMORY_ARCHIVE })];
  for (const board of boards) {
    lines.push(JSON.stringify({ type: 'board', data: board }));
    for (const block of world.blocksOf(board.id)) {
      lines.push(JSON.stringify({ type: 'block', data: block }));
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
