<!--
  Where a picker or a panel of settings appears: a bottom sheet, or a popover
  next to what opened it. The content is the same either way; whoever uses
  this decides which (`sheet`), from the width of the screen and the proposal
  being shown.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';

  import * as Popover from '$lib/ui/popover/index.js';

  import BottomSheet from './BottomSheet.svelte';
  import { coarse, insets } from './layout.svelte.ts';

  interface Props {
    open: boolean;
    sheet: boolean;
    title: string;
    subtitle?: string;
    tall?: boolean;
    /** Classes of the popover: its width, mostly. */
    popoverClass?: string;
    /** The element that opens it. Spread `props` on a button, before its own attributes. */
    trigger: Snippet<[Record<string, unknown>]>;
    children: Snippet;
  }

  let {
    open = $bindable(),
    sheet,
    title,
    subtitle,
    tall = false,
    popoverClass,
    trigger,
    children,
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
  <BottomSheet bind:open {title} {subtitle} {tall}>
    {@render children()}
  </BottomSheet>
{:else}
  <Popover.Root bind:open>
    <Popover.Trigger>
      {#snippet child({ props }: { props: Record<string, unknown> })}
        {@render trigger(props)}
      {/snippet}
    </Popover.Trigger>
    <Popover.Content
      align="start"
      collisionPadding={{
        top: 8 + insets.top,
        right: 8 + insets.right,
        bottom: 8 + insets.bottom,
        left: 8 + insets.left,
      }}
      aria-label={title}
      data-testid="popover"
      class={[
        'max-w-[calc(100vw-1rem-var(--safe-left)-var(--safe-right))] gap-0 p-0',
        popoverClass,
      ]}
      onOpenAutoFocus={(event: Event) => {
        // A finger has not asked for the keyboard yet.
        if (coarse.current) event.preventDefault();
      }}
    >
      {@render children()}
    </Popover.Content>
  </Popover.Root>
{/if}
