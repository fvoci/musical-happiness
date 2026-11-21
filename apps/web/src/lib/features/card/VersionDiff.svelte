<!--
  What changed from one body to another, as one body to read (body-diff.ts):
  how much was added and removed, the way from change to change (the two
  buttons, or Alt+↓ and Alt+↑ anywhere in it), and the merged body, where
  what is the same in both for more than a few blocks is folded away and
  shown on asking. A comparison with no change says so, the body folded.
  Made anew for each comparison (`{#key}` in VersionCompare.svelte): what was
  opened, and the change last come to, are of one comparison.
-->
<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';

  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronUpIcon from '@lucide/svelte/icons/chevron-up';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import DiffBlocks from '$lib/editor/DiffBlocks.svelte';
  import type { ReferenceDrawing } from '$lib/editor/reference-chip.ts';
  import { DIFF_STOP } from '$lib/editor/render-diff.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { foldSame, type BodyDiff } from '$lib/logic/body-diff.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    diff: BodyDiff;
    resolveUser?: (userId: string) => string | undefined;
    references?: ReferenceDrawing;
  }

  let { diff, resolveUser, references }: Props = $props();

  const segments = $derived(foldSame(diff.blocks));
  /** The folded stretches that were opened, by their place: a new comparison makes a new view. */
  const opened = new SvelteSet<number>();
  const counts = $derived(
    [
      m.body_diff_counts({ added: String(diff.added), removed: String(diff.removed) }),
      diff.formatted > 0 ? m.body_diff_formatted({ count: String(diff.formatted) }) : undefined,
      diff.reshaped > 0 ? m.body_diff_reshaped({ count: String(diff.reshaped) }) : undefined,
    ]
      .filter(Boolean)
      .join(' · '),
  );

  let region = $state<HTMLElement>();
  /** The change the way through them came to last: where it goes on from, when the focus is elsewhere. */
  let reached = -1;

  /** Goes to the next change (`1`) or the one before (`-1`), round at the ends, and gives it the focus. */
  function go(step: 1 | -1): void {
    const stops = [...(region?.querySelectorAll<HTMLElement>(`[${DIFF_STOP}]`) ?? [])];
    if (stops.length === 0) return;
    const focused = stops.findIndex((stop) => stop.contains(document.activeElement));
    const from = focused >= 0 ? focused : Math.min(reached, stops.length - 1);
    const next =
      from < 0 ? (step === 1 ? 0 : stops.length - 1) : (from + step + stops.length) % stops.length;
    reached = next;
    const stop = stops[next];
    stop?.focus({ preventScroll: true });
    stop?.scrollIntoView({ block: 'center' });
  }

  function keyed(event: KeyboardEvent): void {
    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    go(event.key === 'ArrowDown' ? 1 : -1);
  }
</script>

<div class="grid gap-3" data-testid="body-diff" onkeydown={keyed} role="presentation">
  {#if diff.changed}
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
      <p class="min-w-0 flex-1 text-xs text-muted-foreground" data-testid="body-diff-counts">
        {counts}
      </p>
      <div class="flex shrink-0 gap-1">
        <IconButton
          label={m.body_diff_previous()}
          aria-keyshortcuts="Alt+ArrowUp"
          data-testid="body-diff-previous"
          onclick={() => {
            go(-1);
          }}
        >
          <ChevronUpIcon aria-hidden="true" />
        </IconButton>
        <IconButton
          label={m.body_diff_next()}
          aria-keyshortcuts="Alt+ArrowDown"
          data-testid="body-diff-next"
          onclick={() => {
            go(1);
          }}
        >
          <ChevronDownIcon aria-hidden="true" />
        </IconButton>
      </div>
    </div>
  {:else}
    <p class="text-sm" role="status" data-testid="body-diff-none">{m.body_diff_none()}</p>
  {/if}
  {#if diff.coarse}
    <p class="text-xs text-muted-foreground" data-testid="body-diff-coarse">
      {m.body_diff_coarse()}
    </p>
  {/if}
  <div class="grid gap-3" bind:this={region}>
    {#each segments as segment, index (index)}
      {#if segment.folded && !opened.has(index)}
        <Button
          variant="outline"
          size="sm"
          class="justify-self-stretch border-dashed text-muted-foreground roomy:min-h-touch"
          data-testid="body-diff-fold"
          onclick={() => {
            opened.add(index);
          }}
        >
          {m.body_diff_unfold({ count: String(segment.blocks.length) })}
        </Button>
      {:else}
        <DiffBlocks blocks={segment.blocks} {resolveUser} {references} class="text-base" />
      {/if}
    {/each}
  </div>
</div>
