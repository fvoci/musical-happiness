<!--
  The search of the workspace's cards and pages, where the features meet:
  its button (in the top bar), its keys (⌘K, Ctrl+K), its dialog, and going
  to what it found by the router.

  Only where the server searches bodies (`searchEnabled`): until that is
  known, and on a server without a search server, there is no button and
  the keys are the browser's. A search answered with 404
  `search_not_configured` means the server's settings changed: the dialog
  closes and the button goes.

  A card opens over the view of its board that is shown, or else over the
  board's first view; a page at its wiki's address. The name of the board or
  wiki of a hit is the one the sidebar knows, none where it does not.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import SearchIcon from '@lucide/svelte/icons/search';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { opensSearch, searchKeysLabel } from '$lib/features/search/search-keys.ts';
  import { SearchSession } from '$lib/features/search/search-session.svelte.ts';
  import { SearchSwitch } from '$lib/features/search/search-switch.svelte.ts';
  import SearchDialog from '$lib/features/search/SearchDialog.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { afterOwnSteps, overlayOpen } from '$lib/layout/back.svelte.ts';
  import { buildUrl } from '$lib/logic/urls.ts';
  import type { Workspace } from '$lib/state/index.ts';
  import type { SearchHit } from '$lib/state/content-search.ts';

  import type { Route, Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  /**
   * The view segment of the address of a card on a board not shown: it names
   * no view, so the board page puts its first view in its place and keeps
   * the card (`BoardPage.svelte`).
   */
  const ANY_VIEW = '-';

  let open = $state(false);
  let session = $state.raw<SearchSession | undefined>(undefined);
  let dialog = $state<{ focusField: () => void }>();
  /** What had the focus when the search opened: it gets it back. */
  let opener: HTMLElement | undefined;

  /** Asked of each backend the workspace has (one, for the life of the page). */
  const enabled = $derived(new SearchSwitch(workspace.backend.search));

  const keys = searchKeysLabel(navigator.userAgent);

  /** Where the focus goes back to: where it was, or the page where that is gone (the button, after a 404). */
  function returnTo(): HTMLElement | undefined {
    if (opener?.isConnected && opener !== document.body) return opener;
    return document.querySelector('main') ?? undefined;
  }

  /** Opens the search, from `from` (the button: Safari does not focus a button clicked). */
  function openSearch(from?: HTMLElement): void {
    const workspaceId = workspace.info?.id;
    if (!enabled.on || !workspaceId) return;
    const focused = document.activeElement;
    opener = from ?? (focused instanceof HTMLElement ? focused : undefined);
    session?.close();
    session = new SearchSession({
      find: (ask) => workspace.backend.search.find(workspaceId, ask),
      onGone: () => {
        enabled.turnOff();
        open = false;
      },
      onRefused: (error) => {
        workspace.report('load', error);
      },
    });
    open = true;
  }

  /**
   * Whether a drawer, a sheet or a dialog other than a card's is open: all are
   * modal (`aria-modal`), and those the back button closes say so too.
   */
  function otherOverlayOpen(): boolean {
    return (
      overlayOpen() ||
      document.querySelector('[aria-modal="true"]:not([data-card-dialog])') !== null
    );
  }

  /**
   * ⌘K and Ctrl+K. Open, they go back to the field (held down, they do
   * nothing more). Over a drawer, a sheet or another dialog they open
   * nothing: going somewhere from the search would leave that open over the
   * new page, and its entry of the history with it. A card is no such thing:
   * the search opens over it.
   */
  function onkeydown(event: KeyboardEvent): void {
    if (!enabled.on || !opensSearch(event)) return;
    if (open) {
      event.preventDefault();
      if (!event.repeat) dialog?.focusField();
      return;
    }
    if (event.repeat || otherOverlayOpen() || !workspace.info?.id) return;
    event.preventDefault();
    openSearch();
  }

  /** The route of a card: over the view shown where its board is shown, else over any. */
  function cardRoute(hit: SearchHit): Partial<Route> {
    const { boardId, viewId } = router.route;
    const here = boardId === hit.containerId && viewId !== undefined && viewId !== ANY_VIEW;
    return {
      workspaceId: workspace.info?.id,
      boardId: hit.containerId,
      viewId: here ? viewId : ANY_VIEW,
      cardId: hit.id,
    };
  }

  function hrefOf(hit: SearchHit): string {
    if (hit.kind === 'card') return router.href(cardRoute(hit));
    const parts = ['w', workspace.info?.id ?? '', 'wikis', hit.containerId, hit.id];
    return buildUrl(`/${parts.map(encodeURIComponent).join('/')}`, router.basePath);
  }

  /**
   * Closes the search, then goes to a hit, once the dialog has given its
   * entry of the history back: the history is then as if the hit was opened
   * from the page. A card of the board shown opens over its view, in place of
   * a card open there; a card of another board opens over that board, as a
   * board chosen in the sidebar. The router knows no wiki yet: a page is gone
   * to as a new page load.
   */
  async function go(hit: SearchHit): Promise<void> {
    const route = cardRoute(hit);
    const href = hrefOf(hit);
    session?.close();
    open = false;
    await tick();
    if (hit.kind === 'page') {
      afterOwnSteps(() => {
        location.assign(href);
      });
    } else if (route.viewId !== ANY_VIEW) {
      router.openCard(hit.id);
    } else {
      router.openBoard({ id: hit.containerId, workspaceId: workspace.info?.id ?? '' });
      router.go(route, { replace: true });
    }
  }

  function containerOf(hit: SearchHit): string {
    const container = workspace.boards.find((board) => board.id === hit.containerId);
    if (!container) return '';
    return container.title || m['Sidebar.untitled-board']();
  }
</script>

<svelte:window {onkeydown} />

{#if enabled.on}
  <IconButton
    label={m.search_open()}
    title={m.search_open_keys({ keys })}
    data-testid="open-search"
    aria-haspopup="dialog"
    aria-keyshortcuts="Meta+K Control+K"
    onclick={(event: MouseEvent & { currentTarget: HTMLButtonElement }) => {
      openSearch(event.currentTarget);
    }}
  >
    <SearchIcon />
  </IconButton>
{/if}

{#if session}
  <SearchDialog
    bind:open={
      () => open,
      (next: boolean) => {
        open = next;
        if (!next) session?.close();
      }
    }
    {session}
    {containerOf}
    {hrefOf}
    bind:this={dialog}
    onGo={(hit: SearchHit) => void go(hit)}
    {returnTo}
  />
{/if}
