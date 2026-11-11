<!--
  Which properties a view shows on its cards: one switch per property of the
  board. A gallery can also hide the titles, and every view but a table can
  show the marks of comments and description.

  Each switch is saved as it is flipped. The list stays open: a reader
  usually flips several.
-->
<script lang="ts">
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';
  import { Switch } from '$lib/ui/switch/index.js';

  import { propertyToggles, withPropertyVisible, type PropertyToggle } from './view-fields.ts';

  let { board, view }: { board: BoardStore; view: BoardView } = $props();

  const toggles = $derived(
    propertyToggles(board.board?.cardProperties ?? [], view.fields.viewType),
  );

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
    <li>
      <label
        class="flex min-h-8 cursor-default items-center justify-between gap-4 rounded-md px-2 text-sm hover:bg-muted roomy:min-h-touch roomy:text-base"
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
    </li>
  {/each}
</ul>
