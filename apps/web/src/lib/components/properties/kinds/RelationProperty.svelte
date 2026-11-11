<!--
  The value of a card relation property: other cards of the same board,
  picked by their titles. Each picked card is a chip with its title; pressing
  the chip opens that card.

  A card is shown only when it is in `cards` (the board's cards that are
  there). The identifier of a deleted card stays in the stored value, so that
  bringing the card back brings the relation back, but it is not drawn, and
  the next change of the value leaves it out.

  The card itself cannot be picked, a card is picked once, and no more than
  RELATION_LIMIT are picked. The list shows at most LIST_LIMIT cards at a
  time, so that a board of thousands of cards does not draw thousands of
  rows: typing narrows it.
-->
<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';

  import PickList from '$lib/components/pick-list/PickList.svelte';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import type { Presentation } from '$lib/components/picker-surface/presentation.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { foldCase } from '$lib/logic/text.ts';
  import { formatRelationValue, RELATION_LIMIT, type Card } from '$lib/logic/wire.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { cn } from '$lib/utils.js';

  interface Props {
    /** The name of the property. */
    name: string;
    /** The card the value belongs to: it cannot name itself. */
    cardId: string;
    /** The identifiers in the stored value, each once. */
    ids: readonly string[];
    /** The board's cards that are there, by identifier. */
    cards: ReadonlyMap<string, Card>;
    /** Cards that may not be picked besides the card itself (they would close a loop). */
    excluded?: ReadonlySet<string>;
    editable: boolean;
    /** Shown where there is no value; empty for nothing. */
    placeholder: string;
    presentation?: Presentation;
    startOpen?: boolean;
    onChange?: (value: string[]) => void;
    /** Opens a card; without it the chips are only text. */
    onOpenCard?: (cardId: string) => void;
    /** See PropertyValueProps. */
    innerTabindex?: 0 | -1;
  }

  let {
    name,
    cardId,
    ids,
    cards,
    excluded,
    editable,
    placeholder,
    presentation = 'popover',
    startOpen = false,
    onChange,
    onOpenCard,
    innerTabindex,
  }: Props = $props();

  /** How many cards the list draws at a time. */
  const LIST_LIMIT = 50;

  // svelte-ignore state_referenced_locally (only where editing starts)
  let open = $state(startOpen);
  let search = $state('');

  /** The picked cards that are there, in the order they were picked. */
  const shown = $derived(ids.filter((id) => cards.has(id)));
  const full = $derived(shown.length >= RELATION_LIMIT);

  const titleOf = (id: string): string => cards.get(id)?.title || m['KanbanCard.untitled']();

  /** The cards that can be picked and match what was typed; the picked ones always can be taken out. */
  const matches = $derived.by(() => {
    const wanted = foldCase(search.trim());
    const found: Card[] = [];
    for (const card of cards.values()) {
      if (card.id === cardId) continue;
      if (excluded?.has(card.id) && !shown.includes(card.id)) continue;
      if (wanted !== '' && !foldCase(card.title).includes(wanted)) continue;
      found.push(card);
    }
    return found;
  });
  const listed = $derived(matches.slice(0, LIST_LIMIT));

  /** Writes the value: only cards that are there, so that the identifiers of deleted ones go. */
  function write(next: readonly string[]): void {
    onChange?.(formatRelationValue(next.filter((id) => cards.has(id) && id !== cardId)));
  }

  function pick(card: Card): void {
    if (shown.includes(card.id)) write(shown.filter((id) => id !== card.id));
    else if (!full) write([...shown, card.id]);
  }

  const chipClass =
    'inline-flex max-w-full min-w-0 items-center gap-1 rounded-md bg-muted px-1.5 py-0.5 text-sm text-foreground';
</script>

{#snippet chips()}
  {#each shown as id (id)}
    {#if onOpenCard}
      <button
        type="button"
        data-slot="relation-chip"
        data-card-id={id}
        data-inner={innerTabindex === undefined ? undefined : ''}
        tabindex={innerTabindex}
        class={cn(chipClass, 'underline-offset-2 hover:underline focus-visible:underline')}
        aria-label={m.property_relation_open({ title: titleOf(id) })}
        onclick={() => {
          onOpenCard(id);
        }}
      >
        <span class="truncate">{titleOf(id)}</span>
      </button>
    {:else}
      <span data-slot="relation-chip" data-card-id={id} class={chipClass}>
        <span class="truncate">{titleOf(id)}</span>
      </span>
    {/if}
  {/each}
{/snippet}

{#if editable}
  <div
    data-slot="property-relation"
    class="flex min-h-8 w-full min-w-0 flex-wrap items-center gap-1 roomy:min-h-touch"
  >
    {@render chips()}
    <PickerSurface
      bind:open
      {presentation}
      title={name}
      onOpenChange={(next) => {
        if (!next) search = '';
      }}
    >
      {#snippet trigger({ props }: { props: Record<string, unknown> })}
        <button
          {...props}
          type="button"
          data-slot="relation-edit"
          class="flex min-h-8 min-w-8 flex-1 items-center rounded-md px-2 py-1 text-left hover:bg-muted aria-expanded:bg-muted roomy:min-h-touch"
        >
          <span class="sr-only">{name}</span>
          {#if shown.length === 0 && placeholder !== ''}
            <span data-slot="property-empty" class="text-muted-foreground">{placeholder}</span>
          {/if}
        </button>
      {/snippet}

      <div class="flex flex-col gap-1">
        {#if shown.length > 0}
          <ul class="m-0 flex list-none flex-wrap gap-1 p-1" aria-label={name}>
            {#each shown as id (id)}
              <li class={cn(chipClass, 'pr-0.5')}>
                <span class="truncate">{titleOf(id)}</span>
                <button
                  type="button"
                  class="relative inline-flex size-5 shrink-0 items-center justify-center rounded-sm group-data-[presentation=sheet]/surface:size-8 group-data-[presentation=sheet]/surface:after:absolute group-data-[presentation=sheet]/surface:after:-inset-1.5 hover:bg-foreground/10"
                  aria-label={m.property_option_remove({ name: titleOf(id) })}
                  onclick={() => {
                    write(shown.filter((other) => other !== id));
                  }}
                >
                  <XIcon class="size-3.5" aria-hidden="true" />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <PickList
          bind:search
          items={listed}
          textOf={(card: Card) => card.title}
          selected={shown}
          multiple
          label={name}
          placeholder={m.property_relation_search()}
          emptyText={m.property_no_matches()}
          onPick={pick}
          onBackspaceEmpty={() => {
            const last = shown.at(-1);
            if (last !== undefined) write(shown.filter((id) => id !== last));
          }}
        >
          {#snippet row(card: Card)}
            <span class="truncate">{card.title || m['KanbanCard.untitled']()}</span>
          {/snippet}
        </PickList>
        {#if matches.length > listed.length}
          <p class="px-2 py-1 text-sm text-muted-foreground" data-slot="relation-more">
            {m.property_relation_more({ count: matches.length - listed.length })}
          </p>
        {/if}
        {#if full}
          <p class="px-2 py-1 text-sm text-muted-foreground" role="status">
            {m.property_relation_full({ count: RELATION_LIMIT })}
          </p>
        {/if}
        {#if shown.length > 0}
          <Button
            variant="ghost"
            class="justify-start group-data-[presentation=sheet]/surface:min-h-touch"
            onclick={() => {
              write([]);
            }}
          >
            {m.property_clear()}
          </Button>
        {/if}
      </div>
    </PickerSurface>
  </div>
{:else}
  <span
    data-slot="property-relation"
    class="flex min-h-8 min-w-0 flex-wrap items-center gap-1 px-2 py-1"
  >
    {@render chips()}
    {#if shown.length === 0 && placeholder !== ''}
      <span data-slot="property-empty" class="text-muted-foreground">{placeholder}</span>
    {/if}
  </span>
{/if}
