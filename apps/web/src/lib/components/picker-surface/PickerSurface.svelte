<!--
  The surface a list of choices or a date picker opens on: a popover next to
  its trigger on a wide screen, a sheet from the bottom edge on a narrow one.

  This is the only place that knows the two. Which one is used is the
  `presentation` argument; the caller decides (default-presentation.svelte.ts
  has a default that follows the width of the viewport). To change how
  pickers open on narrow screens, change the `sheet` branch below.

  Inside the surface, `group-data-[presentation=sheet]/surface:` styles a
  child for the sheet only: rows a finger has to hit grow to `min-h-touch`.

  Where the focus goes on opening differs on purpose. In the popover it goes
  to the element marked `data-picker-focus` (the field of a list), or else to
  the first control. On the sheet it goes to the sheet itself: focusing a
  field there would raise the on-screen keyboard over half of the choices
  before the person has asked to type.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';
  import * as Popover from '$lib/ui/popover/index.js';
  import * as Sheet from '$lib/ui/sheet/index.js';
  import { cn } from '$lib/utils.js';

  import type { Presentation } from './presentation.ts';

  interface Props {
    open?: boolean;
    presentation: Presentation;
    /** Names the surface for assistive technology; the heading of the sheet. */
    title: string;
    /** The element that opens the surface. Spread `props` on a button, before its own attributes. */
    trigger: Snippet<[{ props: Record<string, unknown> }]>;
    children: Snippet;
    onOpenChange?: (open: boolean) => void;
    class?: string;
  }

  let {
    open = $bindable(false),
    presentation,
    title,
    trigger,
    children,
    onOpenChange,
    class: className,
  }: Props = $props();

  let sheet = $state<HTMLElement | null>(null);
  let popover = $state<HTMLElement | null>(null);

  function focusSheet(event: Event): void {
    event.preventDefault();
    sheet?.focus();
  }

  function focusMarked(event: Event): void {
    const marked = popover?.querySelector<HTMLElement>('[data-picker-focus]');
    if (!marked) return;
    event.preventDefault();
    marked.focus();
  }
</script>

{#if presentation === 'sheet'}
  <Sheet.Root bind:open {onOpenChange}>
    <Sheet.Trigger>
      {#snippet child({ props }: { props: Record<string, unknown> })}
        {@render trigger({ props })}
      {/snippet}
    </Sheet.Trigger>
    <Sheet.Content
      bind:ref={sheet}
      side="bottom"
      showCloseButton={false}
      class="max-h-[85dvh] gap-0 rounded-t-xl pb-[env(safe-area-inset-bottom)] outline-none"
      onOpenAutoFocus={focusSheet}
    >
      <Sheet.Header class="pb-2">
        <Sheet.Title>{title}</Sheet.Title>
      </Sheet.Header>
      <div
        data-presentation="sheet"
        class={cn('group/surface min-h-0 flex-1 overflow-y-auto px-2', className)}
      >
        {@render children()}
      </div>
      <Sheet.Footer class="pt-2">
        <Sheet.Close>
          {#snippet child({ props }: { props: Record<string, unknown> })}
            <Button {...props} variant="outline" class="min-h-touch">{m.ui_done()}</Button>
          {/snippet}
        </Sheet.Close>
      </Sheet.Footer>
    </Sheet.Content>
  </Sheet.Root>
{:else}
  <Popover.Root bind:open {onOpenChange}>
    <Popover.Trigger>
      {#snippet child({ props }: { props: Record<string, unknown> })}
        {@render trigger({ props })}
      {/snippet}
    </Popover.Trigger>
    <Popover.Content
      bind:ref={popover}
      align="start"
      aria-label={title}
      class="max-h-(--bits-popover-content-available-height) w-auto max-w-[calc(100vw-1rem)] min-w-64 gap-0 overflow-y-auto p-1"
      onOpenAutoFocus={focusMarked}
    >
      <div data-presentation="popover" class={cn('group/surface', className)}>
        {@render children()}
      </div>
    </Popover.Content>
  </Popover.Root>
{/if}
