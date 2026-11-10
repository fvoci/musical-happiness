<!--
  A menu of commands: a bottom sheet of tall lines, or the dropdown menu next
  to what opened it. Whoever uses this decides which (`sheet`).
-->
<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import type { Snippet } from 'svelte';

  import * as DropdownMenu from '$lib/ui/dropdown-menu/index.js';

  import type { Action } from './actions.ts';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    sheet: boolean;
    title: string;
    subtitle?: string;
    actions: readonly Action[];
    align?: 'start' | 'end';
    /** The element that opens it. Spread `props` on a button, before its own attributes. */
    trigger: Snippet<[Record<string, unknown>]>;
  }

  let {
    open = $bindable(),
    sheet,
    title,
    subtitle,
    actions,
    align = 'end',
    trigger,
  }: Props = $props();
</script>

{#if sheet}
  {@render trigger({
    'aria-haspopup': 'dialog',
    'aria-expanded': open,
    onclick: () => {
      open = true;
    },
  })}
  <BottomSheet bind:open {title} {subtitle}>
    <ul class="px-2 pb-2">
      {#each actions as action (action.id)}
        {@const Icon = action.icon}
        <li class={[action.separated && 'mt-1 border-t pt-1']}>
          <button
            type="button"
            class={[
              'flex min-h-12 w-full items-center gap-3 rounded-lg px-3 text-left text-base hover:bg-muted active:bg-muted',
              action.destructive && 'text-destructive',
            ]}
            onclick={() => {
              open = false;
            }}
          >
            <Icon class="size-5 shrink-0" />
            <span class="min-w-0 flex-1 truncate">{action.label}</span>
            {#if action.hint}
              <span class="shrink-0 text-sm text-muted-foreground">{action.hint}</span>
            {/if}
            {#if action.checked}
              <CheckIcon class="size-5 shrink-0" />
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </BottomSheet>
{:else}
  <DropdownMenu.Root bind:open>
    <DropdownMenu.Trigger>
      {#snippet child({ props }: { props: Record<string, unknown> })}
        {@render trigger(props)}
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content
      {align}
      collisionPadding={8}
      aria-label={title}
      data-testid="dropdown"
      class="w-60 max-w-[calc(100vw-1rem)]"
    >
      {#each actions as action (action.id)}
        {@const Icon = action.icon}
        {#if action.separated}
          <DropdownMenu.Separator />
        {/if}
        <DropdownMenu.Item
          variant={action.destructive ? 'destructive' : 'default'}
          class="pointer-coarse:min-h-touch pointer-coarse:gap-2.5 pointer-coarse:px-2.5"
        >
          <Icon />
          <span class="min-w-0 flex-1 truncate">{action.label}</span>
          {#if action.hint}
            <span class="shrink-0 text-xs text-muted-foreground">{action.hint}</span>
          {/if}
          {#if action.checked}
            <CheckIcon />
          {/if}
        </DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
