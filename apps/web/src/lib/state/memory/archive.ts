/**
 * An archive of boards as the backend in memory writes it: a line of its
 * own version, then each board and its blocks, one per line. What the rooms
 * of their cards took is written into the cards first (`flushBodies`).
 */
import type { Board } from '../../logic/wire.ts';
import type { DownloadedFile, DownloadOptions } from '../backend.ts';

import { flushBodies } from './bodies.ts';
import { MEMORY_ARCHIVE } from './shapes.ts';
import type { World } from './world.ts';

export function archiveOf(
  world: World,
  boards: readonly Board[],
  options: DownloadOptions | undefined,
): DownloadedFile {
  flushBodies(world, boards);
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
