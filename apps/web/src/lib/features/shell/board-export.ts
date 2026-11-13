/** Saving a board as an archive from the sidebar, apart from the list, which is long. */
import DownloadIcon from '@lucide/svelte/icons/download';

import type { MenuAction } from '$lib/components/menu/actions.ts';
import { m } from '$lib/i18n/paraglide/messages.js';
import { saveBlob } from '$lib/save.ts';
import type { Workspace } from '$lib/state/index.ts';

/** Saves an archive of the board, once it has all arrived. */
async function exportBoard(workspace: Workspace, boardId: string): Promise<void> {
  const archive = await workspace.exportBoard(boardId);
  if (archive) saveBlob(archive.bytes, archive.name);
}

/** The entry of a board's menu that saves it as an archive: a guest's too. */
export function exportAction(workspace: Workspace, boardId: string, separated = false): MenuAction {
  return {
    id: 'export',
    label: m.shell_export_board(),
    icon: DownloadIcon,
    separated,
    run: () => void exportBoard(workspace, boardId),
  };
}
