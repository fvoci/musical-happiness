/** Saving a board or a wiki as an archive from the sidebar, apart from the list, which is long. */
import DownloadIcon from '@lucide/svelte/icons/download';

import type { MenuAction } from '$lib/components/menu/actions.ts';
import { m } from '$lib/i18n/paraglide/messages.js';
import { saveBlob } from '$lib/save.ts';
import type { Workspace } from '$lib/state/index.ts';

/** Saves an archive of the board or wiki, once it has all arrived. */
async function exportBoard(workspace: Workspace, boardId: string): Promise<void> {
  const archive = await workspace.exportBoard(boardId);
  if (archive) saveBlob(archive.bytes, archive.name);
}

/** The entry of a board's or a wiki's menu that saves it as an archive: a guest's too. */
export function exportAction(
  workspace: Workspace,
  board: { id: string; kind?: string },
  separated = false,
): MenuAction {
  const boardId = board.id;
  return {
    id: 'export',
    label: board.kind === 'wiki' ? m.shell_export_wiki() : m.shell_export_board(),
    icon: DownloadIcon,
    separated,
    run: () => void exportBoard(workspace, boardId),
  };
}
