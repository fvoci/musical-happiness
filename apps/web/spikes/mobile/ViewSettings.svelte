<!--
  Filter, sort and group of a view. On a wide screen each is a popover of its
  own, opened by its own button (`tabs` off: the filter alone). On a narrow
  screen the three share one sheet with tabs, because the header has room for
  one button, not three.
-->
<script lang="ts">
  import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
  import CheckIcon from '@lucide/svelte/icons/check';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import LayersIcon from '@lucide/svelte/icons/layers';
  import ListFilterIcon from '@lucide/svelte/icons/list-filter';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import XIcon from '@lucide/svelte/icons/x';

  import { Button } from '$lib/ui/button/index.js';
  import * as Tabs from '$lib/ui/tabs/index.js';

  import { FILTERS, GROUPABLE, SORTS } from './data.ts';
  import IconButton from './IconButton.svelte';
  import { T } from './text.ts';

  interface Props {
    tabs: boolean;
  }

  let { tabs }: Props = $props();

  let tab = $state('filter');
  let groupBy = $state<string>(GROUPABLE[0]);
</script>

<!-- Looks like a select and opens nothing: which field it stands for is all that matters here. -->
{#snippet choice(text: string, grow: boolean)}
  <button
    type="button"
    class={[
      'flex h-8 min-w-0 items-center justify-between gap-1 rounded-lg border border-input-border px-2.5 text-left text-sm pointer-coarse:h-touch pointer-coarse:text-base',
      grow ? 'flex-1 basis-40' : 'shrink-0',
    ]}
  >
    <span class="truncate">{text}</span>
    <ChevronDownIcon class="size-4 shrink-0 text-muted-foreground" />
  </button>
{/snippet}

{#snippet filter()}
  <ul class="flex flex-col gap-3">
    {#each FILTERS as entry (entry.property)}
      <li class="flex flex-wrap items-center gap-2">
        {@render choice(entry.property, false)}
        {@render choice(entry.condition, false)}
        <div class="flex min-w-0 flex-1 basis-40 items-center gap-1">
          {@render choice(entry.value, true)}
          <IconButton label={T.removeFilter}><XIcon /></IconButton>
        </div>
      </li>
    {/each}
  </ul>
  <Button variant="ghost" class="mt-2 self-start pointer-coarse:h-touch">
    <PlusIcon data-icon="inline-start" />
    {T.addFilter}
  </Button>
{/snippet}

{#snippet sort()}
  <ul class="flex flex-col gap-3">
    {#each SORTS as entry (entry.property)}
      <li class="flex items-center gap-2">
        {@render choice(entry.property, true)}
        {@render choice(entry.direction, false)}
        <IconButton label={T.removeSort}><XIcon /></IconButton>
      </li>
    {/each}
  </ul>
  <Button variant="ghost" class="mt-2 self-start pointer-coarse:h-touch">
    <PlusIcon data-icon="inline-start" />
    {T.addSort}
  </Button>
{/snippet}

{#snippet group()}
  <ul role="radiogroup" aria-label={T.groupBy} class="flex flex-col">
    {#each GROUPABLE as property (property)}
      <li>
        <button
          type="button"
          role="radio"
          aria-checked={groupBy === property}
          class="flex min-h-9 w-full items-center gap-3 rounded-lg px-3 text-left hover:bg-muted pointer-coarse:min-h-12 pointer-coarse:text-base"
          onclick={() => {
            groupBy = property;
          }}
        >
          <span class="flex-1">{property}</span>
          {#if groupBy === property}
            <CheckIcon class="size-5" />
          {/if}
        </button>
      </li>
    {/each}
  </ul>
{/snippet}

{#if tabs}
  <Tabs.Root bind:value={tab} class="gap-0">
    <div class="px-4 pb-3">
      <!-- A tab is 7 pixels lower than its list (padding, and a pixel the generated tab leaves). -->
      <Tabs.List class="w-full pointer-coarse:h-[calc(var(--spacing-touch)+7px)]!">
        <Tabs.Trigger value="filter" class="pointer-coarse:text-base">
          <ListFilterIcon />
          {T.filter}
        </Tabs.Trigger>
        <Tabs.Trigger value="sort" class="pointer-coarse:text-base">
          <ArrowUpDownIcon />
          {T.sort}
        </Tabs.Trigger>
        <Tabs.Trigger value="group" class="pointer-coarse:text-base">
          <LayersIcon />
          {T.group}
        </Tabs.Trigger>
      </Tabs.List>
    </div>
    <Tabs.Content value="filter" class="flex flex-col px-4 pb-4">{@render filter()}</Tabs.Content>
    <Tabs.Content value="sort" class="flex flex-col px-4 pb-4">{@render sort()}</Tabs.Content>
    <Tabs.Content value="group" class="flex flex-col px-1 pb-3">{@render group()}</Tabs.Content>
  </Tabs.Root>
{:else}
  <div class="flex flex-col p-3">{@render filter()}</div>
{/if}
