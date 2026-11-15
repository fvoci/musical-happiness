<!--
  The search of the workspace's cards and pages, where the features meet:
  its button (in the top bar), its keys (⌘K on Apple's devices, Ctrl+K elsewhere), its dialog, and going
  to what it found by the router.

  Only where the server says it searches bodies (`searchEnabled`, which
  every server does: on its search server, or on its database): until that
  is known there is no button and the keys are the browser's. A search
  server that does not answer, or cannot hold its index, answers each search
  503, which the dialog says.

  A card opens over the view of its board that is shown, or else over the
  board's first view; a page at its wiki's address. The name of the board or
  wiki of a hit is the one the sidebar knows, none where it does not.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import SearchIcon from '@lucide/svelte/icons/search';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import {
    opensSearch,
    searchKeysLabel,
    searchKeysNames,
  } from '$lib/features/search/search-keys.ts';
  import { SearchSession } from '$lib/features/search/search-session.svelte.ts';
  import { SearchSwitch } from '$lib/features/search/search-switch.svelte.ts';
  import SearchDialog from '$lib/features/search/SearchDialog.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { overlayOpen } from '$lib/layout/back.svelte.ts';
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

  const keys = searchKeysLabel();

  /** Where the focus goes back to: where it was, or the page where that is gone. */
  function returnTo(): HTMLElement | undefined {
    if (opener?.isConnected && opener !== document.body) return opener;
    return document.querySelector('main') ?? undefined;
  }

  /**
   * Opens the search, from `from` (the button: Safari does not focus a button
   * clicked); whether it did. Never over another overlay: the wiki's drawer of
   * pages leaves the top bar out of its reach, and going from the search to a
   * page of the same wiki would leave it open, without its entry of the history.
   */
  function openSearch(from?: HTMLElement): boolean {
    const workspaceId = workspace.info?.id;
    if (!enabled.on || !workspaceId || otherOverlayOpen()) return false;
    const focused = document.activeElement;
    // Not the field of the search that was just closed and is fading out: it goes.
    const kept = focused instanceof HTMLElement && !focused.closest('[data-search-dialog]');
    opener = from ?? (kept ? focused : undefined);
    session?.close();
    session = new SearchSession({
      find: (ask) => workspace.backend.search.find(workspaceId, ask),
      onRefused: (error) => {
        workspace.report('load', error);
      },
    });
    open = true;
    return true;
  }

  /**
   * Whether a drawer, a sheet or a dialog other than a card's is open: all are
   * modal (`aria-modal`), and those the back button closes say so too. A
   * dialog closed and still fading out (the search's own, after a hit was
   * chosen) is none.
   */
  function otherOverlayOpen(): boolean {
    const other = '[aria-modal="true"]:not([data-card-dialog], [data-state="closed"])';
    return overlayOpen() || document.querySelector(other) !== null;
  }

  /**
   * ⌘K, or Ctrl+K off Apple's devices. Open, they go back to the field (held down, they do
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
    if (!event.repeat && openSearch()) event.preventDefault();
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

  /** The route of a page: its wiki's address. */
  function pageRoute(hit: SearchHit): Partial<Route> {
    return { workspaceId: workspace.info?.id, wikiId: hit.containerId, pageId: hit.id };
  }

  function hrefOf(hit: SearchHit): string {
    return router.href(hit.kind === 'card' ? cardRoute(hit) : pageRoute(hit));
  }

  /**
   * Closes the search, then goes to a hit, once the dialog has given its
   * entry of the history back: the history is then as if the hit was opened
   * from the page. A card of the board shown opens over its view, in place of
   * a card open there; a card of another board opens over that board, as a
   * board chosen in the sidebar. A page of the wiki shown is opened as one of
   * its tree; a page of another wiki over that wiki, as a wiki chosen in the
   * sidebar.
   */
  async function go(hit: SearchHit): Promise<void> {
    const route = hit.kind === 'card' ? cardRoute(hit) : pageRoute(hit);
    session?.close();
    open = false;
    await tick();
    if (hit.kind === 'page' && router.route.wikiId === hit.containerId) {
      router.openPage(hit.id);
    } else if (hit.kind === 'card' && route.viewId !== ANY_VIEW) {
      router.openCard(hit.id);
    } else {
      const kind = hit.kind === 'page' ? 'wiki' : 'board';
      router.openBoard({ id: hit.containerId, workspaceId: workspace.info?.id ?? '', kind });
      router.go(route, { replace: true });
    }
    await tick();
    settleFocus();
  }

  /**
   * After going: the focus, still in the field fading out (and then on
   * nothing), goes to the card shown, else to the page. A card that opens
   * later, over another board, takes it from there.
   */
  function settleFocus(): void {
    const at = document.activeElement;
    if (at instanceof HTMLElement && at !== document.body && !at.closest('[data-search-dialog]')) {
      return;
    }
    const card = document.querySelector<HTMLElement>('[data-card-dialog]');
    (card ?? document.querySelector<HTMLElement>('main'))?.focus();
  }

  function containerOf(hit: SearchHit): string {
    const container = workspace.boards.find((board) => board.id === hit.containerId);
    if (!container) return '';
    if (container.title) return container.title;
    return container.kind === 'wiki' ? m.wiki_untitled() : m['Sidebar.untitled-board']();
  }
</script>

<svelte:window {onkeydown} />

{#if enabled.on}
  <IconButton
    label={m.search_open()}
    title={m.search_open_keys({ keys })}
    data-testid="open-search"
    aria-haspopup="dialog"
    aria-keyshortcuts={searchKeysNames()}
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
