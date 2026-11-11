<!--
  Which properties a view shows on its cards: one switch per property of the
  board. A gallery can also hide the titles, and every view but a table can
  show the marks of comments and description.

  Each switch is saved as it is flipped. The list stays open: a reader
  usually flips several. Who may change the board's properties can move one
  up or down: that is the board's order, which every view and card follows.
-->
<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { movedProperty } from '$lib/logic/property-order.ts';
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import { Switch } from '$lib/ui/switch/index.js';

  import { propertyToggles, withPropertyVisible, type PropertyToggle } from './view-fields.ts';

  let { board, view }: { board: BoardStore; view: BoardView } = $props();

  const toggles = $derived(propertyToggles(board.board?.cardProperties ?? []));
  const order = $derived(board.board?.cardProperties.map((template) => template.id) ?? []);

  function move(propertyId: string, step: -1 | 1): void {
    void board.reorderProperties(movedProperty(order, propertyId, step));
  }

  function nameOf(toggle: PropertyToggle): string {
    return toggle.template.name;
  }

  function show(propertyId: string, visible: boolean): void {
    void board.setViewFields(
      view.id,
      {
        visiblePropertyIds: withPropertyVisible(
          view.fields.visiblePropertyIds,
          propertyId,
          visible,
        ),
      },
      'showProperty',
    );
  }
</script>

<ul class="m-0 flex list-none flex-col p-1" data-testid="properties-panel">
  {#each toggles as toggle (toggle.id)}
    {@const name = nameOf(toggle)}
    <li class="flex items-center gap-1">
      <label
        class="flex min-h-8 min-w-0 flex-1 cursor-default items-center justify-between gap-4 rounded-md px-2 text-sm hover:bg-muted roomy:min-h-touch roomy:text-base"
      >
        <span class="min-w-0 truncate">{name}</span>
        <Switch
          aria-label={name}
          data-property={toggle.id}
          bind:checked={
            () => view.fields.visiblePropertyIds.includes(toggle.id),
            (visible: boolean) => {
              show(toggle.id, visible);
            }
          }
        />
      </label>
      {#if board.canEdit && order.includes(toggle.id)}
        {@const place = order.indexOf(toggle.id)}
        <IconButton
          label={m.view_property_move_up({ name })}
          class="size-7 text-muted-foreground"
          data-action="move-up"
          disabled={place === 0}
          onclick={() => {
            move(toggle.id, -1);
          }}
        >
          <ArrowUpIcon aria-hidden="true" />
        </IconButton>
        <IconButton
          label={m.view_property_move_down({ name })}
          class="size-7 text-muted-foreground"
          data-action="move-down"
          disabled={place === order.length - 1}
          onclick={() => {
            move(toggle.id, 1);
          }}
        >
          <ArrowDownIcon aria-hidden="true" />
        </IconButton>
      {/if}
    </li>
  {/each}
</ul>
