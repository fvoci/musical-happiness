/** The title a board or a wiki is shown under: its own, or the words for one without. */
import { m } from '$lib/i18n/paraglide/messages.js';
import type { Container } from '$lib/logic/wire.ts';

export function shownTitle(board: Pick<Container, 'title' | 'kind'>): string {
  if (board.title) return board.title;
  return board.kind === 'wiki' ? m.wiki_untitled() : m['Sidebar.untitled-board']();
}
