<!--
  The search of what cards and pages say: a field, the kind to find (all,
  cards, pages), and the list of what was found. In the middle of the page;
  on a compact page it fills the screen.

  The field is a combobox and keeps the focus: the arrow keys choose a row
  of the list, Enter goes to it, ⌘/Ctrl+Enter opens it in a new tab, and
  Escape (or the back button) closes the dialog, the focus going back to
  where it was (`returnTo`). While an input method puts letters together,
  Enter and the arrows are its own. How many were found, and that a search
  is on its way or found nothing, is read out as it changes.
-->
<script lang="ts">
  import RotateCwIcon from '@lucide/svelte/icons/rotate-cw';
  import SearchIcon from '@lucide/svelte/icons/search';

  import { arrowChoice } from '$lib/components/arrow-choice.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import { MAX_SEARCH_CHARS, type SearchHit, type SearchKind } from '$lib/state/content-search.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import { listMove } from './search-keys.ts';
  import type { SearchSession } from './search-session.svelte.ts';
  import SearchHitRow from './SearchHitRow.svelte';

  interface Props {
    open: boolean;
    session: SearchSession;
    /** The name of the board or wiki a hit is in; empty where it is not known. */
    containerOf: (hit: SearchHit) => string;
    hrefOf: (hit: SearchHit) => string;
    /** Goes to a hit in this tab. The dialog closes. */
    onGo: (hit: SearchHit) => void;
    /** Where the focus goes back to when the dialog closes without going anywhere. */
    returnTo: () => HTMLElement | undefined;
  }

  let { open = $bindable(), session, containerOf, hrefOf, onGo, returnTo }: Props = $props();

  const uid = $props.id();
  const listId = `${uid}-list`;
  const optionId = (index: number): string => `${uid}-hit-${index}`;

  let field = $state<HTMLInputElement | null>(null);
  let list = $state<HTMLElement | null>(null);
  /** The row chosen, by its kind and identifier: the first when it is no longer there. */
  let chosen = $state<string | undefined>(undefined);
  /** The dialog closes because a hit was gone to: the focus is the new page's. */
  let went = false;

  const keyOf = (hit: SearchHit): string => `${hit.kind}:${hit.id}`;
  const hits = $derived(session.hits);
  const activeIndex = $derived(
    Math.max(
      0,
      hits.findIndex((hit) => keyOf(hit) === chosen),
    ),
  );
  /** The list is shown with what was found, and while a new search for it is on its way. */
  const listed = $derived(
    (session.status === 'found' || session.status === 'searching') && hits.length > 0,
  );
  const compact = $derived(page.stage === 'compact');

  const KINDS: { kind: SearchKind | undefined; label: () => string }[] = [
    { kind: undefined, label: () => m.search_kind_all() },
    { kind: 'card', label: () => m.search_kind_cards() },
    { kind: 'page', label: () => m.search_kind_pages() },
  ];

  /** What is read out of the state of the search. */
  const said = $derived.by(() => {
    switch (session.status) {
      case 'searching':
        return m.search_searching();
      case 'found':
        return m.search_found({ count: hits.length });
      case 'none':
        return m.search_none();
      case 'limited':
        return m.search_limited();
      case 'unavailable':
        return m.search_unavailable();
      case 'failed':
        return m.search_failed();
      default:
        return '';
    }
  });

  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );

  function choose(index: number): void {
    const hit = hits[index];
    if (!hit) return;
    chosen = keyOf(hit);
    list?.querySelector(`#${CSS.escape(optionId(index))}`)?.scrollIntoView({ block: 'nearest' });
  }

  function go(hit: SearchHit): void {
    went = true;
    onGo(hit);
  }

  /** Puts the focus in the field, what it holds chosen: ⌘K again while the dialog is open. */
  export function focusField(): void {
    field?.focus();
    field?.select();
  }

  /**
   * The keys of the list act on what the words in the field found: not while
   * a search for newer words is on its way, whose list is still the last one.
   */
  function onkeydown(event: KeyboardEvent): void {
    const move = listMove(event);
    if (move === undefined || session.status !== 'found' || hits.length === 0) return;
    event.preventDefault();
    const hit = hits[activeIndex];
    if (move === 'next') choose((activeIndex + 1) % hits.length);
    else if (move === 'previous') choose((activeIndex - 1 + hits.length) % hits.length);
    else if (!hit) return;
    else if (move === 'go') go(hit);
    else window.open(hrefOf(hit), '_blank', 'noopener');
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    data-testid="search-dialog"
    data-status={session.status}
    data-compact={compact}
    showCloseButton={compact}
    class={[
      'flex flex-col gap-3 p-3',
      compact
        ? 'inset-0 h-dvh max-w-none translate-x-0 translate-y-0 rounded-none pt-[calc(var(--safe-top)+0.75rem)] pr-[calc(var(--safe-right)+0.75rem)] pb-(--safe-bottom) pl-[calc(var(--safe-left)+0.75rem)] ring-0'
        : 'top-[12dvh] max-h-[76dvh] translate-y-0 sm:max-w-xl',
    ]}
    onOpenAutoFocus={(event: Event) => {
      event.preventDefault();
      went = false;
      field?.focus();
    }}
    onCloseAutoFocus={(event: Event) => {
      event.preventDefault();
      if (!went) returnTo()?.focus();
    }}
  >
    <Dialog.Title class="sr-only">{m.search_title()}</Dialog.Title>
    <Dialog.Description class="sr-only">{m.search_description()}</Dialog.Description>

    <div class={['relative', compact && 'pr-10']}>
      <SearchIcon
        class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
        aria-hidden="true"
      />
      <input
        bind:this={field}
        type="search"
        role="combobox"
        data-testid="search-field"
        aria-label={m.search_field()}
        aria-expanded={listed}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={listed ? optionId(activeIndex) : undefined}
        autocomplete="off"
        spellcheck="false"
        enterkeyhint="go"
        maxlength={MAX_SEARCH_CHARS}
        placeholder={m.search_placeholder()}
        value={session.query}
        class="h-10 w-full min-w-0 rounded-lg border border-input-border bg-transparent pr-2.5 pl-8.5 text-base outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"
        oninput={(event) => {
          session.type(event.currentTarget.value);
        }}
        {onkeydown}
      />
    </div>

    <div
      role="radiogroup"
      aria-label={m.search_kinds()}
      class="flex gap-1"
      data-testid="search-kinds"
      {@attach arrowChoice()}
    >
      {#each KINDS as each (each.kind ?? 'all')}
        <button
          type="button"
          role="radio"
          aria-checked={session.kind === each.kind}
          data-kind={each.kind ?? 'all'}
          class="rounded-full border px-3 py-1 text-xs aria-checked:border-primary aria-checked:bg-primary aria-checked:text-primary-foreground roomy:min-h-touch"
          onclick={() => {
            session.choose(each.kind);
          }}
        >
          {each.label()}
        </button>
      {/each}
    </div>

    <p class="sr-only" aria-live="polite" data-testid="search-said">{said}</p>

    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
      <div
        bind:this={list}
        id={listId}
        role="listbox"
        aria-label={m.search_results()}
        aria-busy={session.status === 'searching'}
        hidden={!listed}
        class="grid gap-0.5"
      >
        {#each hits as hit, index (keyOf(hit))}
          <SearchHitRow
            {hit}
            id={optionId(index)}
            href={hrefOf(hit)}
            container={containerOf(hit)}
            active={listed && index === activeIndex}
            onGo={() => {
              go(hit);
            }}
            onPoint={() => {
              chosen = keyOf(hit);
            }}
          />
        {/each}
      </div>

      {#if session.status === 'empty'}
        <p class="px-2 py-3 text-muted-foreground" data-testid="search-hint">{m.search_hint()}</p>
      {:else if session.status === 'searching' && !listed}
        <p class="px-2 py-3 text-muted-foreground" aria-hidden="true">{m.search_searching()}</p>
      {:else if session.status === 'none'}
        <p class="px-2 py-3 text-muted-foreground" aria-hidden="true">{m.search_none()}</p>
      {:else if session.status === 'limited'}
        <p class="px-2 py-3 text-muted-foreground" aria-hidden="true">{m.search_limited()}</p>
      {:else if session.status === 'unavailable' || session.status === 'failed'}
        <div class="grid justify-items-center gap-2 px-2 py-4 text-center">
          <p aria-hidden="true">
            {session.status === 'unavailable' ? m.search_unavailable() : m.search_failed()}
          </p>
          <Button
            variant="outline"
            class="roomy:min-h-touch"
            data-testid="search-retry"
            onclick={() => {
              session.retry();
              field?.focus();
            }}
          >
            <RotateCwIcon aria-hidden="true" />
            {m.app_retry()}
          </Button>
        </div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
