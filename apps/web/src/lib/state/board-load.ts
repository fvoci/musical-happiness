/** Reads a board's content and learns the names it carries beyond current membership. */
import { authorIds, memberId } from './user-names.ts';
import { boardOfWorkspace } from './board-parts.ts';
import type { Workspace } from './workspace.svelte.ts';

export async function readBoard(workspace: Workspace, boardId: string) {
  const backend = workspace.backend;
  const visit = workspace.info;
  const [board, views, cards, members] = await Promise.all([
    boardOfWorkspace(backend, boardId, visit?.id),
    backend.views(boardId),
    backend.cards(boardId),
    backend.members(boardId),
  ]);
  if (workspace.info === visit) {
    void workspace.learnUsers([board.createdBy, ...authorIds(cards)]);
    void workspace.learnUsers(members.map(memberId), { members: true });
  }
  return [board, views, cards, members] as const;
}
