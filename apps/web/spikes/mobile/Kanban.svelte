<!--
  The board as columns of cards.

  It follows the width of the pane it is in, not of the window: the same
  board is one column wide beside an open card and four columns wide alone.

  Where two columns fit side by side (`@columns/pane`) they are 16rem wide,
  17rem in a roomy pane, and the board scrolls sideways freely when there are
  more than fit. In a pane narrower than that, two proposals (`paging`):

  - `snap`: one column at a time. A column is nearly as wide as the pane, a
    sliver of its neighbours shows on either side, and the board comes to rest
    on a column. A row of tabs above names the columns, says which is in view
    and jumps to one.
  - `free`: the same 16rem columns, scrolled freely. One column and a third
    of the next are in view.

  The sizes are classes with container variants. Script only needs to know
  whether the board is snapping, to put the right column in view.

  Nothing is dragged here; docs/spike-drag.md has that. What matters for it:
  a swipe that starts on a card scrolls, in both proposals.
-->
<script lang="ts">
  import ArrowRightLeftIcon from '@lucide/svelte/icons/arrow-right-left';
  import CalendarIcon from '@lucide/svelte/icons/calendar';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LayoutTemplateIcon from '@lucide/svelte/icons/layout-template';
  import LinkIcon from '@lucide/svelte/icons/link';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { untrack } from 'svelte';

  import ActionMenu from './ActionMenu.svelte';
  import type { Action } from './actions.ts';
  import Avatar from './Avatar.svelte';
  import Chip from './Chip.svelte';
  import {
    PRIORITIES,
    STATUSES,
    cardsIn,
    optionOf,
    personOf,
    statusOf,
    type Card,
  } from './data.ts';
  import IconButton from './IconButton.svelte';
  import { pane } from './layout.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    paging: 'snap' | 'free';
    /** The column in view when the page loads (`snap` only). */
    column: number;
    /** The card whose menu is open when the page loads. */
    menuCard: string | undefined;
    /** Menus are bottom sheets. */
    sheet: boolean;
    onopen: (card: Card) => void;
  }

  let { paging, column, menuCard, sheet, onopen }: Props = $props();

  const snapping = $derived(paging === 'snap' && pane.measured && !pane.columns);

  let scroller = $state<HTMLElement | null>(null);
  let current = $state(untrack(() => Math.min(column, STATUSES.length - 1)));
  let menuOf = $state(untrack(() => menuCard));

  function columnElements(): HTMLElement[] {
    return [...(scroller?.querySelectorAll<HTMLElement>('[data-column]') ?? [])];
  }

  /** Where the board has to be scrolled to for a column to sit in the middle. */
  function restOf(element: HTMLElement, within: HTMLElement): number {
    return element.offsetLeft - (within.clientWidth - element.clientWidth) / 2;
  }

  // The column asked for is in view from the first frame: no animation to wait
  // for. Again whenever the pane narrows to one column: the one last looked at.
  $effect(() => {
    if (!snapping || !scroller) return;
    const element = columnElements()[untrack(() => current)];
    if (element) scroller.scrollLeft = restOf(element, scroller);
  });

  function onScroll(): void {
    if (!snapping || !scroller) return;
    const within = scroller;
    let nearest = 0;
    let least = Infinity;
    columnElements().forEach((element, at) => {
      const distance = Math.abs(restOf(element, within) - within.scrollLeft);
      if (distance < least) {
        least = distance;
        nearest = at;
      }
    });
    current = nearest;
  }

  function show(at: number): void {
    columnElements()[at]?.scrollIntoView({
      behavior: 'smooth',
      inline: 'center',
      block: 'nearest',
    });
  }

  function actionsOf(card: Card): Action[] {
    return [
      { id: 'duplicate', label: T.duplicate, icon: CopyIcon },
      { id: 'link', label: T.copyLink, icon: LinkIcon },
      // Without dragging: the way to another column that works with one thumb.
      { id: 'move', label: T.moveToColumn, icon: ArrowRightLeftIcon, hint: statusOf(card).label },
      { id: 'template', label: T.saveAsTemplate, icon: LayoutTemplateIcon },
      { id: 'delete', label: T.deleteCard, icon: TrashIcon, destructive: true, separated: true },
    ];
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  {#if paging === 'snap'}
    <div
      role="tablist"
      aria-label={T.columns}
      data-testid="column-tabs"
      class="@columns/pane:hidden no-scrollbar flex shrink-0 overflow-x-auto border-b px-2"
    >
      {#each STATUSES as status, at (status.id)}
        <button
          type="button"
          role="tab"
          aria-selected={at === current}
          class="flex h-9 shrink-0 items-center px-1 pointer-coarse:h-touch"
          onclick={() => {
            show(at);
          }}
        >
          <span
            class={[
              'flex h-8 items-center gap-1.5 rounded-full px-3 text-sm whitespace-nowrap',
              at === current
                ? 'bg-primary font-medium text-primary-foreground'
                : 'text-muted-foreground',
            ]}
          >
            {status.label}
            <span class="text-xs opacity-80">{cardsIn(status.id).length}</span>
          </span>
        </button>
      {/each}
    </div>
  {/if}

  <div
    bind:this={scroller}
    data-testid="kanban"
    class={[
      'flex min-h-0 flex-1 items-start gap-3 overflow-x-auto overflow-y-hidden py-3',
      paging === 'snap'
        ? '@columns/pane:snap-none @columns/pane:px-3 @header/pane:px-4 snap-x snap-mandatory px-7'
        : '@header/pane:px-4 px-3',
    ]}
    onscroll={onScroll}
  >
    {#each STATUSES as status (status.id)}
      {@const cards = cardsIn(status.id)}
      <section
        data-column={status.id}
        aria-label={status.label}
        class={[
          'flex max-h-full shrink-0 flex-col rounded-lg bg-muted',
          paging === 'snap'
            ? '@columns/pane:w-64 @columns/pane:max-w-none @columns/pane:snap-align-none @header/pane:w-68 w-full max-w-sm snap-center'
            : '@header/pane:w-68 w-64',
        ]}
      >
        <header class="flex items-center gap-2 pt-1 pr-1 pl-3">
          <Chip option={status} />
          <span class="text-xs text-muted-foreground">{cards.length}</span>
          <div class="flex-1"></div>
          <IconButton label={T.newCardIn(status.label)}><PlusIcon /></IconButton>
          <IconButton label={T.columnMenu(status.label)}><EllipsisIcon /></IconButton>
        </header>
        <div class="flex min-h-0 flex-col gap-2 overflow-y-auto overscroll-y-contain p-2">
          {#each cards as card (card.id)}
            {@const priority = optionOf(PRIORITIES, card.priority)}
            {@const assignee = personOf(card.assignee)}
            <div
              data-card={card.id}
              role="button"
              tabindex="0"
              class="group/card relative shrink-0 cursor-pointer rounded-md border bg-card p-3 text-card-foreground shadow-xs select-none"
              onclick={(event) => {
                if (!(event.target as Element).closest('[data-card-menu]')) onopen(card);
              }}
              onkeydown={(event) => {
                if (event.key === 'Enter' && event.target === event.currentTarget) onopen(card);
              }}
            >
              <p class="pr-7 font-medium pointer-coarse:pr-9">{card.title}</p>
              {#if Boolean(priority) || Boolean(card.due) || Boolean(assignee)}
                <div class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1">
                  {#if priority}
                    <Chip option={priority} />
                  {/if}
                  {#if card.due}
                    <span class="flex items-center gap-1 text-xs text-muted-foreground">
                      <CalendarIcon class="size-3.5" />
                      {card.due}
                    </span>
                  {/if}
                  <div class="flex-1"></div>
                  {#if assignee}
                    <Avatar person={assignee} />
                  {/if}
                </div>
              {/if}
              <!--
                Where the device can hover, the button appears when the card
                is pointed at. A finger cannot point without pressing, so
                there it is always shown. Asked of the device, not of the width.
              -->
              <div
                data-card-menu
                class="absolute top-1 right-1 pointer-coarse:top-0 pointer-coarse:right-0"
              >
                <ActionMenu
                  bind:open={
                    () => menuOf === card.id, (open) => (menuOf = open ? card.id : undefined)
                  }
                  {sheet}
                  title={T.cardMenu}
                  subtitle={card.title}
                  actions={actionsOf(card)}
                >
                  {#snippet trigger(props: Record<string, unknown>)}
                    <IconButton
                      {...props}
                      label={T.cardMenu}
                      class="can-hover:opacity-0 can-hover:group-hover/card:opacity-100 can-hover:focus-visible:opacity-100 can-hover:aria-expanded:opacity-100 text-muted-foreground"
                    >
                      <EllipsisIcon />
                    </IconButton>
                  {/snippet}
                </ActionMenu>
              </div>
            </div>
          {/each}
          <button
            type="button"
            class="flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2 text-left text-sm text-muted-foreground hover:bg-background/60 pointer-coarse:h-touch"
          >
            <PlusIcon class="size-4" />
            {T.newCard}
          </button>
        </div>
      </section>
    {/each}
    <button
      type="button"
      class="@columns/pane:flex hidden h-9 w-68 shrink-0 items-center gap-1.5 rounded-lg px-3 text-left text-sm text-muted-foreground hover:bg-muted"
    >
      <PlusIcon class="size-4" />
      {T.addGroup}
    </button>
  </div>
</div>
