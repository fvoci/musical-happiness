<!--
  A menu of commands: the dropdown next to what opened it, or on a compact
  page a bottom sheet of tall lines. Which one is the `presentation`
  argument; without it the stage of the page decides
  (lib/layout/presentation.ts, the one place to change the rule).

  The sheet names what the menu is about (`title`, and `subtitle` for the
  card or board it belongs to), because it opens away from what was pressed.
-->
<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import type { Snippet } from 'svelte';

  import { overlayPresentation, type OverlayPresentation } from '$lib/layout/presentation.ts';
  import { insets, page } from '$lib/layout/room.svelte.ts';
  import * as DropdownMenu from '$lib/ui/dropdown-menu/index.js';

  import BottomSheet from '../sheet/BottomSheet.svelte';

  import type { MenuAction } from './actions.ts';

  interface Props {
    open?: boolean;
    title: string;
    subtitle?: string;
    actions: readonly MenuAction[];
    align?: 'start' | 'end';
    presentation?: OverlayPresentation;
    /** The element that opens it. Spread `props` on a button, before its own attributes. */
    trigger: Snippet<[Record<string, unknown>]>;
  }

  let {
    open = $bindable(false),
    title,
    subtitle,
    actions,
    align = 'end',
    presentation,
    trigger,
  }: Props = $props();

  const shown = $derived(presentation ?? overlayPresentation(page.stage));

  function choose(action: MenuAction): void {
    open = false;
    action.run();
  }
</script>

{#if shown === 'sheet'}
  {@render trigger({
    'aria-haspopup': 'dialog',
    'aria-expanded': open,
    onclick: () => {
      open = true;
    },
  })}
  <BottomSheet bind:open {title} {subtitle}>
    <ul class="px-2 pb-2" role="menu" aria-label={title}>
      {#each actions as action (action.id)}
        {@const Icon = action.icon}
        <li role="none" class={[action.separated && 'mt-1 border-t pt-1']}>
          <button
            type="button"
            role="menuitem"
            data-action={action.id}
            disabled={action.disabled}
            class={[
              'flex min-h-9 w-full items-center gap-3 rounded-lg px-3 text-left hover:bg-muted active:bg-muted disabled:opacity-50 roomy:min-h-12 roomy:text-base',
              action.destructive && 'text-destructive',
            ]}
            onclick={() => {
              choose(action);
            }}
          >
            {#if Icon}<Icon class="size-5 shrink-0" />{/if}
            <span class="min-w-0 flex-1 truncate">{action.label}</span>
            {#if action.hint}
              <span class="shrink-0 text-sm text-muted-foreground">{action.hint}</span>
            {/if}
            {#if action.checked}
              <CheckIcon class="size-5 shrink-0" aria-hidden="true" />
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
      collisionPadding={insets.collisionPadding}
      aria-label={title}
      data-testid="dropdown"
      class="w-60 max-w-[calc(100vw-1rem-var(--safe-left)-var(--safe-right))]"
    >
      {#each actions as action (action.id)}
        {@const Icon = action.icon}
        {#if action.separated}
          <DropdownMenu.Separator />
        {/if}
        <DropdownMenu.Item
          variant={action.destructive ? 'destructive' : 'default'}
          disabled={action.disabled}
          data-action={action.id}
          class="roomy:min-h-touch roomy:gap-2.5 roomy:px-2.5"
          onSelect={() => {
            action.run();
          }}
        >
          {#if Icon}<Icon />{/if}
          <span class="min-w-0 flex-1 truncate">{action.label}</span>
          {#if action.hint}
            <span class="shrink-0 text-xs text-muted-foreground">{action.hint}</span>
          {/if}
          {#if action.checked}
            <CheckIcon aria-hidden="true" />
          {/if}
        </DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
