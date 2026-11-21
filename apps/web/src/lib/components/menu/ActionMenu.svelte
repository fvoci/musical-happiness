<!--
  A menu of commands: the dropdown next to what opened it, or on a compact
  page a bottom sheet of tall lines. Which one is the `presentation`
  argument; without it the stage of the page decides
  (lib/layout/presentation.ts, the one place to change the rule).

  The sheet names what the menu is about (`title`, and `subtitle` for the
  card or board it belongs to), because it opens away from what was pressed.

  A line with an address (`href`) is a link, in either: it can be opened in
  a new tab, and the one marked is the page shown (`aria-current`).
-->
<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check';
  import type { Snippet } from 'svelte';

  import { overlayPresentation, type OverlayPresentation } from '$lib/layout/presentation.ts';
  import { insets, page } from '$lib/layout/room.svelte.ts';
  import * as DropdownMenu from '$lib/ui/dropdown-menu/index.js';

  import BottomSheet from '../sheet/BottomSheet.svelte';

  import { followedHere, type MenuAction } from './actions.ts';

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
  /** What opened the sheet, for the focus to come back to: Safari does not focus a pressed button. */
  let opener: HTMLElement | null = null;

  function choose(action: MenuAction): void {
    open = false;
    action.run();
  }

  /** A link's line pressed: answered here, or (a new tab or window asked for) left to the browser. */
  function follow(event: MouseEvent, action: MenuAction): void {
    if (!followedHere(event)) return;
    event.preventDefault();
    action.run();
  }

  const SHEET_LINE =
    'flex min-h-9 w-full items-center gap-3 rounded-lg px-3 text-left hover:bg-muted active:bg-muted disabled:opacity-50 roomy:min-h-12 roomy:text-base';
</script>

{#snippet sheetLine(action: MenuAction)}
  {@const Icon = action.icon}
  {#if Icon}<Icon class="size-5 shrink-0" />{/if}
  <span class="min-w-0 flex-1 truncate">{action.label}</span>
  {#if action.hint}
    <span class="shrink-0 text-sm text-muted-foreground">{action.hint}</span>
  {/if}
  {#if action.checked}
    <CheckIcon class="size-5 shrink-0" aria-hidden="true" />
  {/if}
{/snippet}

{#snippet dropdownLine(action: MenuAction)}
  {@const Icon = action.icon}
  {#if Icon}<Icon />{/if}
  <span class="min-w-0 flex-1 truncate">{action.label}</span>
  {#if action.hint}
    <span class="shrink-0 text-xs text-muted-foreground">{action.hint}</span>
  {/if}
  {#if action.checked}
    <CheckIcon aria-hidden="true" />
  {/if}
{/snippet}

{#if shown === 'sheet'}
  {@render trigger({
    'aria-haspopup': 'dialog',
    'aria-expanded': open,
    // In the capture phase: a trigger may put its own `onclick` in place of this one's.
    onclickcapture: (event: MouseEvent) => {
      opener = event.currentTarget as HTMLElement;
    },
    onclick: () => {
      open = true;
    },
  })}
  <BottomSheet bind:open {title} {subtitle} returnFocus={() => opener}>
    <ul class="px-2 pb-2" role="menu" aria-label={title}>
      {#each actions as action (action.id)}
        <li role="none" class={[action.separated && 'mt-1 border-t pt-1']}>
          {#if action.href === undefined}
            <button
              type="button"
              role="menuitem"
              aria-label={action.ariaLabel}
              data-action={action.id}
              disabled={action.disabled}
              class={[SHEET_LINE, action.destructive && 'text-destructive']}
              onclick={() => {
                choose(action);
              }}
            >
              {@render sheetLine(action)}
            </button>
          {:else}
            <a
              href={action.href}
              role="menuitem"
              aria-label={action.ariaLabel}
              aria-current={action.checked ? 'page' : undefined}
              data-action={action.id}
              class={SHEET_LINE}
              onclick={(event) => {
                open = false;
                follow(event, action);
              }}
            >
              {@render sheetLine(action)}
            </a>
          {/if}
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
        {#if action.separated}
          <DropdownMenu.Separator />
        {/if}
        {#if action.href === undefined}
          <DropdownMenu.Item
            variant={action.destructive ? 'destructive' : 'default'}
            disabled={action.disabled}
            aria-label={action.ariaLabel}
            data-action={action.id}
            class="roomy:min-h-touch roomy:gap-2.5 roomy:px-2.5"
            onSelect={() => {
              action.run();
            }}
          >
            {@render dropdownLine(action)}
          </DropdownMenu.Item>
        {:else}
          {@const href = action.href}
          <!-- Enter is a click on the link (Bits UI): it runs, and the menu closes. -->
          <DropdownMenu.Item
            aria-label={action.ariaLabel}
            data-action={action.id}
            class="roomy:min-h-touch roomy:gap-2.5 roomy:px-2.5"
          >
            {#snippet child({ props }: { props: Record<string, unknown> })}
              {@const select = props.onclick as ((event: MouseEvent) => void) | undefined}
              <a
                {...props}
                {href}
                aria-current={action.checked ? 'page' : undefined}
                onclick={(event) => {
                  select?.(event);
                  follow(event, action);
                }}
              >
                {@render dropdownLine(action)}
              </a>
            {/snippet}
          </DropdownMenu.Item>
        {/if}
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
