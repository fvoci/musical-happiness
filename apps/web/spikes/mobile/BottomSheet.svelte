<!--
  A sheet that rises from the bottom edge: what a menu or a picker becomes in
  a compact window.

  - Never taller than the screen less a strip at the top, so that what is
    behind it stays recognisable and there is somewhere to tap to dismiss it.
  - Its content scrolls inside; the title stays put.
  - Sits on top of the on-screen keyboard (`--keyboard`) and clear of the home
    indicator (`--safe-bottom`).
  - Opening it moves the focus to the sheet itself, not to its first field: a
    focused field would bring up the keyboard before the person has seen the
    choices.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';

  import * as Sheet from '$lib/ui/sheet/index.js';

  import { closeOnBack } from './history.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    open: boolean;
    title: string;
    /** Under the title: what the sheet is about, such as the name of the card. */
    subtitle?: string;
    /** Fills the height it is allowed even when its content is shorter. */
    tall?: boolean;
    children: Snippet;
  }

  let { open = $bindable(), title, subtitle, tall = false, children }: Props = $props();

  let content = $state<HTMLElement | null>(null);

  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );
</script>

<Sheet.Root bind:open>
  <Sheet.Content
    bind:ref={content}
    side="bottom"
    showCloseButton={false}
    data-testid="bottom-sheet"
    class={[
      'max-h-[calc(100dvh-var(--safe-top)-var(--keyboard)-3rem)] gap-0 rounded-t-2xl pr-(--safe-right) pb-(--safe-bottom) pl-(--safe-left) outline-none data-[side=bottom]:bottom-(--keyboard)',
      tall && 'data-[side=bottom]:h-dvh',
    ]}
    onOpenAutoFocus={(event: Event) => {
      event.preventDefault();
      content?.focus();
    }}
  >
    <!-- Says "this can be pulled down"; dragging it is not part of the mock-up. -->
    <div
      aria-hidden="true"
      class="mx-auto mt-2 h-1 w-9 shrink-0 rounded-full bg-input-border"
    ></div>
    <Sheet.Header class="shrink-0 flex-row items-center gap-2 py-1 pr-1 pl-4">
      <div class="min-w-0 flex-1">
        <Sheet.Title class="truncate pr-0">{title}</Sheet.Title>
        {#if subtitle}
          <Sheet.Description class="truncate text-xs text-muted-foreground">
            {subtitle}
          </Sheet.Description>
        {/if}
      </div>
      <Sheet.Close
        class="h-8 shrink-0 rounded-lg px-3 text-sm font-medium text-foreground hover:bg-muted pointer-coarse:h-touch"
      >
        {T.done}
      </Sheet.Close>
    </Sheet.Header>
    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
      {@render children()}
    </div>
  </Sheet.Content>
</Sheet.Root>
