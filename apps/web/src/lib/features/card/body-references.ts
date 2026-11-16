/**
 * What the bodies of a board's cards, and of a wiki's pages, are given to
 * draw and to put their references with (`BodyReferences`): the titles the
 * workspace holds (`referenceTitles`), the places the app leads to, and what
 * `[[` offers.
 */
import type { BodyReferences } from '$lib/editor/body-editor.ts';
import { m } from '$lib/i18n/paraglide/messages.js';
import {
  referenceTitles,
  type BoardStore,
  type LinkedBlock,
  type Workspace,
} from '$lib/state/index.ts';

/** Where the bodies are: a board, whose cards `[[` offers before anything is typed, or a wiki. */
export type BodiesPlace = BoardStore | { workspace: Workspace };

/** What the list after `[[` says of a card or a page: its icon and title, and where it is when that is elsewhere. */
function offered(place: BodiesPlace, linked: LinkedBlock): string {
  const name = `${linked.icon === '' ? '' : `${linked.icon} `}${linked.title || m.editor_reference_untitled()}`;
  if (linked.kind === 'page') return `${name} · ${m.editor_reference_page()}`;
  if ('boardId' in place && linked.containerId === place.boardId) return name;
  const where = place.workspace.boards.find((each) => each.id === linked.containerId);
  return where ? `${name} · ${where.title || m['Sidebar.untitled-board']()}` : name;
}

/** What is drawn of the references in the bodies of `place`; `around` is the card or the page the bodies are of, which `[[` does not offer. */
export function bodyReferences(place: BodiesPlace, around?: string): BodyReferences {
  const board = 'realCards' in place ? place : undefined;
  const titles = referenceTitles(place.workspace);
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
        .map((linked) => ({ value: linked.id, label: offered(place, linked) }));
    },
    named: (address) => titles.places?.named(address),
    seen: (id) => titles.seen(id),
  };
}
