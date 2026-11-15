/**
 * The wiki on the page, for the parts of the screen that are not its own (the
 * sidebar asks what the reader may do with it), as `workspace.board` is the
 * board on the page. Kept beside the workspace for as long as it is.
 */
import type { WikiStore } from './wiki.svelte.ts';
import type { Workspace } from './workspace.svelte.ts';

export class ShownWiki {
  current = $state.raw<WikiStore | undefined>(undefined);
}

const shown = new WeakMap<Workspace, ShownWiki>();

export function shownWiki(workspace: Workspace): ShownWiki {
  let holder = shown.get(workspace);
  if (!holder) {
    holder = new ShownWiki();
    shown.set(workspace, holder);
  }
  return holder;
}
