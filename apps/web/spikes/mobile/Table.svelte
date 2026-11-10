<!--
  The board as a table: a row a card, a column a property.

  Where the pane is wide it is a plain table. The title column widens with
  the pane (10rem, 14rem, 22rem). In a pane too narrow for two kanban columns,
  two proposals (`narrowAs`):

  - `table`: still a table. The title column stays at the left edge while the
    others scroll under it sideways, and the header row stays at the top.
  - `list`: no columns. Every row becomes a block: the title in full, the
    shown properties in a line under it. Nothing scrolls sideways.
-->
<script lang="ts">
  import CalendarIcon from '@lucide/svelte/icons/calendar';
  import CircleDotIcon from '@lucide/svelte/icons/circle-dot';
  import ClockIcon from '@lucide/svelte/icons/clock';
  import FlagIcon from '@lucide/svelte/icons/flag';
  import TagIcon from '@lucide/svelte/icons/tag';
  import TypeIcon from '@lucide/svelte/icons/type';
  import UserIcon from '@lucide/svelte/icons/user';
  import { untrack } from 'svelte';

  import type { IconComponent } from './actions.ts';
  import Avatar from './Avatar.svelte';
  import Chip from './Chip.svelte';
  import { CARDS, PRIORITIES, TAGS, optionOf, personOf, statusOf, type Card } from './data.ts';
  import { pane } from './layout.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    narrowAs: 'table' | 'list';
    /** How far the table is scrolled sideways when the page loads, in pixels. */
    scrollX: number;
    onopen: (card: Card) => void;
  }

  let { narrowAs, scrollX, onopen }: Props = $props();

  const COLUMNS: { name: string; icon: IconComponent }[] = [
    { name: T.status, icon: CircleDotIcon },
    { name: T.assignee, icon: UserIcon },
    { name: T.due, icon: CalendarIcon },
    { name: T.priority, icon: FlagIcon },
    { name: T.tags, icon: TagIcon },
    { name: T.hours, icon: ClockIcon },
  ];

  const totalHours = CARDS.reduce((sum, card) => sum + (card.hours ?? 0), 0);

  let scroller = $state<HTMLElement | null>(null);
  let scrolled = $state(false);

  $effect(() => {
    if (scroller) scroller.scrollLeft = untrack(() => scrollX);
  });
</script>

{#if narrowAs === 'list' && pane.measured && !pane.columns}
  <ul data-testid="table-list" class="h-full divide-y overflow-y-auto overscroll-contain">
    {#each CARDS as card (card.id)}
      {@const priority = optionOf(PRIORITIES, card.priority)}
      {@const assignee = personOf(card.assignee)}
      <li>
        <button
          type="button"
          class="flex w-full flex-col gap-1.5 px-4 py-3 text-left hover:bg-muted active:bg-muted"
          onclick={() => {
            onopen(card);
          }}
        >
          <span class="text-base font-medium">{card.title}</span>
          <span class="flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xs text-muted-foreground">
            <Chip option={statusOf(card)} />
            {#if priority}
              <Chip option={priority} />
            {/if}
            {#if card.due}
              <span class="flex items-center gap-1">
                <CalendarIcon class="size-3.5" />
                {card.due}
              </span>
            {/if}
            {#if assignee}
              <span class="flex items-center gap-1">
                <Avatar person={assignee} decorative class="size-5" />
                {assignee.name}
              </span>
            {/if}
          </span>
        </button>
      </li>
    {/each}
  </ul>
{:else}
  <div
    bind:this={scroller}
    data-testid="table"
    class="h-full overflow-auto overscroll-contain"
    onscroll={() => {
      scrolled = (scroller?.scrollLeft ?? 0) > 0;
    }}
  >
    <table
      data-scrolled={scrolled ? '' : undefined}
      class="group/table w-max min-w-full border-separate border-spacing-0 text-sm"
    >
      <thead>
        <tr class="text-left text-muted-foreground">
          <th
            scope="col"
            class="sticky top-0 left-0 z-20 h-9 border-r border-b bg-background px-3 font-medium group-data-scrolled/table:shadow-[6px_0_8px_-6px_rgb(0_0_0/0.35)] pointer-coarse:h-touch"
          >
            <span class="flex items-center gap-1.5">
              <TypeIcon class="size-4" />
              {T.title}
            </span>
          </th>
          {#each COLUMNS as column (column.name)}
            {@const Icon = column.icon}
            <th
              scope="col"
              class="sticky top-0 z-10 border-b bg-background px-3 font-medium whitespace-nowrap"
            >
              <span class="flex items-center gap-1.5">
                <Icon class="size-4" />
                {column.name}
              </span>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each CARDS as card (card.id)}
          {@const priority = optionOf(PRIORITIES, card.priority)}
          {@const assignee = personOf(card.assignee)}
          <tr>
            <th
              scope="row"
              class="sticky left-0 z-10 border-r border-b bg-background p-0 text-left font-normal group-data-scrolled/table:shadow-[6px_0_8px_-6px_rgb(0_0_0/0.35)]"
            >
              <button
                type="button"
                class="@columns/pane:w-56 @header/pane:w-88 block h-9 w-40 truncate px-3 text-left hover:bg-muted pointer-coarse:h-touch"
                onclick={() => {
                  onopen(card);
                }}
              >
                {card.title}
              </button>
            </th>
            <td class="border-b px-3"><Chip option={statusOf(card)} /></td>
            <td class="border-b px-3 whitespace-nowrap">
              {#if assignee}
                <span class="flex items-center gap-1.5">
                  <Avatar person={assignee} decorative class="size-5" />
                  {assignee.name}
                </span>
              {/if}
            </td>
            <td class="border-b px-3 whitespace-nowrap">{card.due}</td>
            <td class="border-b px-3">
              {#if priority}
                <Chip option={priority} />
              {/if}
            </td>
            <td class="border-b px-3">
              <span class="flex gap-1">
                {#each card.tags as tag (tag)}
                  {@const option = optionOf(TAGS, tag)}
                  {#if option}
                    <Chip {option} />
                  {/if}
                {/each}
              </span>
            </td>
            <td class="border-b px-3 text-right whitespace-nowrap tabular-nums">
              {#if card.hours !== undefined}
                {T.hoursOf(card.hours)}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
      <tfoot>
        <tr class="text-xs text-muted-foreground">
          <td
            class="sticky bottom-0 left-0 z-20 h-9 border-t border-r bg-background px-3 group-data-scrolled/table:shadow-[6px_0_8px_-6px_rgb(0_0_0/0.35)]"
          >
            {T.countOf(CARDS.length)}
          </td>
          <td colspan="5" class="sticky bottom-0 z-10 border-t bg-background"></td>
          <td
            class="sticky bottom-0 z-10 border-t bg-background px-3 text-right whitespace-nowrap tabular-nums"
          >
            {T.sumOf(totalHours)}
          </td>
        </tr>
      </tfoot>
    </table>
  </div>
{/if}
