/**
 * What the bodies of a board's cards are given to draw and to put their
 * references with (`BodyReferences`): the titles the workspace holds
 * (`referenceTitles`), the places the app leads to, and what `[[` offers.
 */
import type { BodyReferences } from '$lib/editor/body-editor.ts';
import { m } from '$lib/i18n/paraglide/messages.js';
import { referenceTitles, type BoardStore, type LinkedBlock } from '$lib/state/index.ts';

/** What the list after `[[` says of a card or a page: its icon and title, and where it is when that is elsewhere. */
function offered(board: BoardStore, linked: LinkedBlock): string {
  const name = `${linked.icon === '' ? '' : `${linked.icon} `}${linked.title || m.editor_reference_untitled()}`;
  if (linked.kind === 'page') return `${name} · ${m.editor_reference_page()}`;
  if (linked.boardId === board.boardId) return name;
  const where = board.workspace.boards.find((each) => each.id === linked.boardId);
  return where ? `${name} · ${where.title || m['Sidebar.untitled-board']()}` : name;
}

/** What is drawn of the references in the bodies of `board`; `around` is the card the bodies are of, which `[[` does not offer. */
export function bodyReferences(board: BoardStore, around?: string): BodyReferences {
  const titles = referenceTitles(board.workspace);
  return {
    shown: (id) => {
      const linked = titles.of(id);
      if (!linked) return linked;
      const { title, icon, kind } = linked;
      return { title, icon, kind, href: titles.places?.href(linked) };
    },
    open: (id) => {
      const linked = titles.of(id);
      if (linked) titles.places?.open(linked);
    },
    search: async (query) => {
      const found = await titles.search(query, board);
      return found
        .filter((linked) => linked.id !== around)
        .map((linked) => ({ value: linked.id, label: offered(board, linked) }));
    },
    named: (address) => titles.places?.named(address),
    seen: (id) => titles.seen(id),
  };
}
