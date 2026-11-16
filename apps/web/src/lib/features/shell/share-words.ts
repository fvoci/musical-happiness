/** The words of the dialogs of a board's or a wiki's people that name which of the two it is. */
import { m } from '$lib/i18n/paraglide/messages.js';
import type { ContainerKind } from '$lib/logic/wire.ts';

export function shareWords(kind: ContainerKind | undefined) {
  const wiki = kind === 'wiki';
  return {
    title: wiki ? m.share_wiki_title : m['ShareBoard.Title'],
    workspaceOpen: wiki ? m.share_wiki_workspace_open : m.share_workspace_open,
    guestHint: wiki ? m.share_wiki_guest_hint : m.share_guest_hint,
    guestLinks: wiki ? m.share_wiki_guest_links_title : m.share_guest_links_title,
    guestAddresses: wiki ? m.mail_wiki_guest_addresses_hint : m.mail_guest_addresses_hint,
    teamRemove: wiki ? m.teams_wiki_remove : m.teams_board_remove,
  };
}
