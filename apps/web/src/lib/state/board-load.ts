/** Reads a board's content and learns the names it carries beyond current membership. */
import { boardOfWorkspace } from './board-parts.ts';
import type { Workspace } from './workspace.svelte.ts';

export async function readBoard(workspace: Workspace, boardId: string) {
  const backend = workspace.backend;
  const [board, views, cards, members] = await Promise.all([
    boardOfWorkspace(backend, boardId, workspace.info?.id),
    backend.views(boardId),
    backend.cards(boardId),
    backend.members(boardId),
  ]);
  void workspace.learnUsers(
    [board.createdBy, ...members.map((member) => member.userId)].filter((id) => id !== ''),
  );
  return [board, views, cards, members] as const;
}
