<!--
  Going to another board by its name. A command palette in the middle
  (Ctrl K); in a compact window a tall bottom sheet with the same field and
  the same list, grouped by category.

  As the second proposal for the sidebar this sheet is the only way to the
  list of boards in a compact window: there is no drawer, and the name of
  the board in the top bar opens it.

  The palette is in the middle of the page, which on a folding screen is
  where it folds. docs/mobile-proposal.md lists it among what lies on that
  line.
-->
<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SearchIcon from '@lucide/svelte/icons/search';

  import * as Command from '$lib/ui/command/index.js';
  import { Input } from '$lib/ui/input/index.js';

  import BoardIcon from './BoardIcon.svelte';
  import BottomSheet from './BottomSheet.svelte';
  import { BOARD, CATEGORIES } from './data.ts';
  import { app } from './layout.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();

  let query = $state('');

  const shown = $derived(
    CATEGORIES.map((category) => ({
      ...category,
      boards: category.boards.filter((board) => board.name.includes(query.trim())),
    })).filter((category) => category.boards.length > 0),
  );
</script>

{#if app.stage === 'compact'}
  <BottomSheet bind:open title={T.switchBoard} tall>
    <div class="flex flex-col px-2 pb-2">
      <div class="sticky top-0 z-10 bg-popover px-2 pb-2">
        <SearchIcon
          class="pointer-events-none absolute top-2 left-5 size-4 text-muted-foreground pointer-coarse:top-3.5"
        />
        <Input
          bind:value={query}
          type="text"
          enterkeyhint="search"
          autocomplete="off"
          aria-label={T.findBoard}
          placeholder={T.findBoard}
          class="pl-8 pointer-coarse:h-touch"
        />
      </div>
      {#each shown as category (category.id)}
        <h3 class="px-3 pt-3 pb-1 text-xs font-medium text-muted-foreground">{category.name}</h3>
        <ul>
          {#each category.boards as board (board.id)}
            <li>
              <button
                type="button"
                aria-current={board.id === BOARD.id ? 'page' : undefined}
                class="flex min-h-9 w-full items-center gap-3 rounded-lg px-3 text-left hover:bg-muted active:bg-muted aria-[current]:bg-muted aria-[current]:font-medium pointer-coarse:min-h-12 pointer-coarse:text-base"
                onclick={() => {
                  open = false;
                }}
              >
                <BoardIcon {board} />
                <span class="min-w-0 flex-1 truncate">{board.name}</span>
                {#if board.id === BOARD.id}
                  <CheckIcon class="size-5 shrink-0" />
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="px-3 py-6 text-center text-muted-foreground">{T.noBoardFound}</p>
      {/each}
      <div class="mt-2 border-t pt-1">
        <button
          type="button"
          class="flex min-h-9 w-full items-center gap-3 rounded-lg px-3 text-left hover:bg-muted pointer-coarse:min-h-12 pointer-coarse:text-base"
        >
          <PlusIcon class="size-5 shrink-0" />
          {T.addBoard}
        </button>
      </div>
    </div>
  </BottomSheet>
{:else}
  <Command.Dialog
    bind:open
    title={T.switchBoard}
    description={T.switchBoardHint}
    class="left-[calc(50%+(var(--safe-left)-var(--safe-right))/2)] max-w-[min(32rem,calc(100%-2rem-var(--safe-left)-var(--safe-right)))] sm:max-w-[min(32rem,calc(100%-2rem-var(--safe-left)-var(--safe-right)))]"
  >
    <Command.Input placeholder={T.findBoard} />
    <Command.List class="max-h-96">
      <Command.Empty>{T.noBoardFound}</Command.Empty>
      {#each CATEGORIES as category (category.id)}
        <Command.Group heading={category.name}>
          {#each category.boards as board (board.id)}
            <Command.Item
              value={board.name}
              data-checked={board.id === BOARD.id}
              onSelect={() => {
                open = false;
              }}
            >
              <BoardIcon {board} />
              <span class="min-w-0 flex-1 truncate">{board.name}</span>
            </Command.Item>
          {/each}
        </Command.Group>
      {/each}
    </Command.List>
  </Command.Dialog>
{/if}
