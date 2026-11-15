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
  import SearchIcon from '@lucide/svelte/icons/search';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { opensSearch, searchKeysLabel } from '$lib/features/search/search-keys.ts';
  import { SearchSession } from '$lib/features/search/search-session.svelte.ts';
  import { SearchSwitch } from '$lib/features/search/search-switch.svelte.ts';
  import SearchDialog from '$lib/features/search/SearchDialog.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { releaseOverlayEntries } from '$lib/layout/back.svelte.ts';
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
  let returnTo = $state.raw<HTMLElement | undefined>(undefined);

  /** Asked of each backend the workspace has (one, for the life of the page). */
  const enabled = $derived(new SearchSwitch(workspace.backend.search));

  const keys = searchKeysLabel(navigator.userAgent);

  function openSearch(): void {
    const workspaceId = workspace.info?.id;
    if (!enabled.on || !workspaceId) return;
    const focused = document.activeElement;
    returnTo = focused instanceof HTMLElement ? focused : undefined;
    session?.close();
    session = new SearchSession({
      find: (ask) => workspace.backend.search.find(workspaceId, ask),
      onGone: () => {
        enabled.turnOff();
        open = false;
      },
    });
    open = true;
  }

  function onkeydown(event: KeyboardEvent): void {
    if (!enabled.on || !opensSearch(event)) return;
    event.preventDefault();
    if (!open) {
      openSearch();
      return;
    }
    const field = document.querySelector<HTMLInputElement>('[data-testid="search-field"]');
    field?.focus();
    field?.select();
  }

  /** The route of a card: over the view shown where its board is shown, else over any. */
  function cardRoute(hit: SearchHit): Partial<Route> {
    const { workspaceId, boardId, viewId } = router.route;
    const here = boardId === hit.containerId && viewId !== undefined;
    return {
      workspaceId,
      boardId: hit.containerId,
      viewId: here ? viewId : ANY_VIEW,
      cardId: hit.id,
    };
  }

  function hrefOf(hit: SearchHit): string {
    if (hit.kind === 'card') return router.href(cardRoute(hit));
    const parts = ['w', router.route.workspaceId ?? '', 'wikis', hit.containerId, hit.id];
    return buildUrl(`/${parts.map(encodeURIComponent).join('/')}`, router.basePath);
  }

  /**
   * Goes to a hit, then closes: the new address takes the place of the
   * dialog's entry of the history, as the router does it from the drawer.
   * The router knows no wiki yet: a page is gone to as a new page load.
   */
  function go(hit: SearchHit): void {
    if (hit.kind === 'card') {
      const route = cardRoute(hit);
      if (route.viewId === router.route.viewId) router.openCard(hit.id);
      else router.go(route);
    } else if (releaseOverlayEntries() > 0) {
      location.replace(hrefOf(hit));
    } else {
      location.assign(hrefOf(hit));
    }
    session?.close();
    open = false;
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
    onclick={openSearch}
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
    onGo={go}
    {returnTo}
  />
{/if}
