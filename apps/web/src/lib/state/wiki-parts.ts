/**
 * What the store of an open wiki (`wiki.svelte.ts`) leans on: making a wiki,
 * and a step of undo on a page that is gone since.
 */
import type { Container } from '../logic/wire.ts';

import { isBackendError } from './backend.ts';
import type { WikiDraft } from './wiki-types.ts';
import type { Workspace } from './workspace.svelte.ts';

/** A step of undo on a page deleted since (by the reader too): nothing is left to do. */
export async function unlessGone(step: () => Promise<void>): Promise<void> {
  try {
    await step();
  } catch (error) {
    if (!isBackendError(error) || error.status !== 404) throw error;
  }
}

/**
 * Makes a wiki in the workspace shown, in the sidebar at once; undefined when
 * it was refused (told) or the backend has no wikis.
 */
export async function createWiki(
  workspace: Workspace,
  draft: WikiDraft,
): Promise<Container | undefined> {
  const wikis = workspace.backend.wiki;
  const shown = workspace.info;
  if (!wikis || !shown) return undefined;
  try {
    const made = await wikis.createWiki(shown.id, draft);
    workspace.receiveBoard(made);
    return made;
  } catch (error) {
    workspace.report('save', error);
    return undefined;
  }
}
