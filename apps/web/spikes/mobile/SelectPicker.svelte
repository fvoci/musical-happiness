<!--
  Choosing the value of a select property: a field that narrows the options
  (and names a new one), then the options. The same content in a bottom sheet
  and in a popover; only the height of a line follows the pointer.

  The field is 16 pixels of text where a finger is used: under that, iOS zooms
  the page when the field is focused.
-->
<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import SearchIcon from '@lucide/svelte/icons/search';
  import { untrack } from 'svelte';

  import { Input } from '$lib/ui/input/index.js';

  import Chip from './Chip.svelte';
  import type { Option } from './data.ts';
  import IconButton from './IconButton.svelte';
  import { T } from './text.ts';

  interface Props {
    options: readonly Option[];
    selected: string | undefined;
    /** In a sheet there is more room around the list, and a line for a finger is taller still. */
    roomy: boolean;
    onselect: (id: string | undefined) => void;
  }

  let { options, selected, roomy, onselect }: Props = $props();

  let query = $state('');
  let chosen = $state(untrack(() => selected));

  const shown = $derived(options.filter((option) => option.label.includes(query.trim())));
</script>

<div class={['flex flex-col', roomy ? 'px-2 pb-2' : 'p-1.5']}>
  <div class={['relative', roomy ? 'px-2 pb-2' : 'pb-1.5']}>
    <SearchIcon
      class={[
        'pointer-events-none absolute top-2 size-4 text-muted-foreground pointer-coarse:top-3.5',
        roomy ? 'left-5' : 'left-2.5',
      ]}
    />
    <Input
      bind:value={query}
      type="text"
      enterkeyhint="done"
      autocomplete="off"
      aria-label={T.searchOptions}
      placeholder={T.searchOptions}
      class="pl-8 pointer-coarse:h-touch"
    />
  </div>
  <ul role="listbox" aria-label={T.options}>
    {#each shown as option (option.id)}
      <li class="flex items-center">
        <button
          type="button"
          role="option"
          aria-selected={chosen === option.id}
          class={[
            'flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 text-left hover:bg-muted active:bg-muted',
            roomy ? 'min-h-8 pointer-coarse:min-h-12' : 'min-h-8 pointer-coarse:min-h-touch',
          ]}
          onclick={() => {
            chosen = option.id;
            onselect(option.id);
          }}
        >
          <Chip {option} />
          <span class="flex-1"></span>
          {#if chosen === option.id}
            <CheckIcon class="size-4.5 shrink-0" />
          {/if}
        </button>
        <IconButton label={T.editOption(option.label)} class="shrink-0 text-muted-foreground">
          <EllipsisIcon />
        </IconButton>
      </li>
    {/each}
  </ul>
  {#if shown.length === 0}
    <p class="px-2 py-3 text-sm text-muted-foreground">{T.createOption(query.trim())}</p>
  {/if}
  <div class="mt-1 border-t pt-1">
    <button
      type="button"
      class={[
        'flex w-full items-center rounded-lg px-2 text-left text-sm text-muted-foreground hover:bg-muted',
        roomy ? 'min-h-8 pointer-coarse:min-h-12' : 'min-h-8 pointer-coarse:min-h-touch',
      ]}
      onclick={() => {
        chosen = undefined;
        onselect(undefined);
      }}
    >
      {T.clearValue}
    </button>
  </div>
</div>
