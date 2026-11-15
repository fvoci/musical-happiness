/**
 * Who else has a page of the wiki open now (the wiki's presence), and how
 * that is said: by the faces at the head of the page, and in the name of its
 * row of the tree, which a screen reader says in place of what is inside it.
 */
import { m } from '$lib/i18n/paraglide/messages.js';
import type { WikiStore } from '$lib/state/index.ts';

/** The identifiers of the people other than the reader on the page, as the presence has them. */
export function viewersOf(wiki: WikiStore, pageId: string): string[] {
  return wiki.presence.others
    .filter((person) => person.cardIds.includes(pageId))
    .map((person) => person.userId);
}

export function nameOf(wiki: WikiStore, userId: string): string {
  return wiki.workspace.userName(userId) ?? m['centerPanel.unknown-user']();
}

/** "2 viewing this page: …"; empty when nobody else is on it. */
export function viewersLabel(wiki: WikiStore, userIds: readonly string[]): string {
  if (userIds.length === 0) return '';
  return m.wiki_page_viewers({
    count: userIds.length,
    names: userIds.map((userId) => nameOf(wiki, userId)).join(', '),
  });
}
