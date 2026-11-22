/**
 * Where a diary may come by a way other than its own route, as the server has
 * it (`check_place`): an archive imported, a diary brought back from the
 * trash. Only into the reader's own space, no more than it may hold, and each
 * day of a diary's entries once and not far ahead. A refusal comes before
 * anything is stored.
 */
import { addDays, dateOfUtc, parseCalendarDate } from '../../logic/dates.ts';
import type { Container } from '../../logic/wire.ts';
import { DAYS_AHEAD, DIARIES_PER_SPACE } from './diaries.ts';
import { fail } from './refuse.ts';
import type { StoredBlock, World } from './world.ts';

/** `more` diaries come into `workspaceId`: it is the reader's own space, with room for them. */
export function checkDiaryRoom(
  world: World,
  userId: string,
  workspaceId: string,
  more: number,
): void {
  if (more === 0) return;
  if (world.ownSpaces.get(userId)?.id !== workspaceId) {
    fail(400, 'a diary is kept in its owner’s own space alone', { reason: 'diary_not_own_space' });
  }
  const there = [...world.containers.values()].filter(
    (container) => container.workspaceId === workspaceId && container.kind === 'diary',
  );
  if (there.length + more > DIARIES_PER_SPACE) {
    fail(400, 'the space has as many diaries as it may', {
      reason: 'too_many_diaries',
      limit: DIARIES_PER_SPACE,
    });
  }
}

/** The containers of an archive come into `workspaceId`: its diaries, and the days of their entries. */
export function checkDiariesComing(
  world: World,
  userId: string,
  workspaceId: string,
  coming: { containers: readonly Container[]; blocks: readonly StoredBlock[] },
): void {
  const diaries = coming.containers.filter((container) => container.kind === 'diary');
  checkDiaryRoom(world, userId, workspaceId, diaries.length);
  const latest = addDays(dateOfUtc(world.now()), DAYS_AHEAD);
  for (const diary of diaries) {
    const days = new Set<string>();
    for (const block of coming.blocks) {
      if (block.containerId !== diary.id || block.type !== 'entry') continue;
      const date = block.fields.date;
      const day = parseCalendarDate(typeof date === 'string' ? date : block.title);
      if (!day || day > latest) fail(400, 'an entry’s day is no day, or too far ahead');
      if (days.has(day)) fail(400, 'a diary has one entry a day');
      days.add(day);
    }
  }
}
