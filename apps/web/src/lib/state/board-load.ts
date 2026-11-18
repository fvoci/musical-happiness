/** Reads a board's content and learns the names it carries beyond current membership. */
import { assignedIds, authorIds, memberId } from './user-names.ts';
import { containerOfWorkspace } from './board-parts.ts';
import type { Workspace } from './workspace.svelte.ts';

export async function readBoard(workspace: Workspace, boardId: string) {
  const backend = workspace.backend;
  const visit = workspace.info;
  const [board, views, cards, members] = await Promise.all([
    containerOfWorkspace(backend, boardId, visit?.id),
    backend.views(boardId),
    backend.cards(boardId),
    backend.members(boardId),
  ]);
  if (workspace.info === visit) {
    // One request for the authors, whom the cards are set to and the members together;
    // only the members are asked again.
    const memberIds = members.map(memberId);
    const named = [...authorIds(cards), ...assignedIds(cards, board.cardProperties)];
    void workspace.learnUsers([board.createdBy, ...named, ...memberIds], { members: memberIds });
  }
  return [board, views, cards, members] as const;
}
