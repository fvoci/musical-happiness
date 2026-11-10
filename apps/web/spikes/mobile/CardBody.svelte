<!--
  What a card is made of, top to bottom: title, properties, body, attachments,
  comments. The frame around it (page, dialog, sheet) is CardDetail's.

  A property is a line: its name, then its value as one button as wide as the
  rest of the line. The whole value is the target, not the text in it.
-->
<script lang="ts">
  import CalendarIcon from '@lucide/svelte/icons/calendar';
  import CircleDotIcon from '@lucide/svelte/icons/circle-dot';
  import ClockIcon from '@lucide/svelte/icons/clock';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import FileTextIcon from '@lucide/svelte/icons/file-text';
  import FlagIcon from '@lucide/svelte/icons/flag';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SquareIcon from '@lucide/svelte/icons/square';
  import SquareCheckIcon from '@lucide/svelte/icons/square-check';
  import TagIcon from '@lucide/svelte/icons/tag';
  import UserIcon from '@lucide/svelte/icons/user';
  import { untrack, type Snippet } from 'svelte';

  import { Button } from '$lib/ui/button/index.js';

  import type { IconComponent } from './actions.ts';
  import Avatar from './Avatar.svelte';
  import Chip from './Chip.svelte';
  import {
    ATTACHMENTS,
    BODY,
    COMMENTS,
    PRIORITIES,
    STATUSES,
    TAGS,
    optionOf,
    personOf,
    type Card,
  } from './data.ts';
  import DatePicker from './DatePicker.svelte';
  import IconButton from './IconButton.svelte';
  import SelectPicker from './SelectPicker.svelte';
  import Surface from './Surface.svelte';
  import { T } from './text.ts';

  interface Props {
    card: Card;
    /** The property whose editor is open when the page loads. */
    editing: 'status' | 'due' | undefined;
    /** Editors are bottom sheets. */
    sheet: boolean;
  }

  let { card, editing, sheet }: Props = $props();

  let status = $state(untrack(() => card.status));
  let statusOpen = $state(untrack(() => editing === 'status'));
  let dueOpen = $state(untrack(() => editing === 'due'));

  const statusOption = $derived(optionOf(STATUSES, status));
  const priority = $derived(optionOf(PRIORITIES, card.priority));
  const assignee = $derived(personOf(card.assignee));

  const VALUE =
    'flex min-h-8 w-full min-w-0 items-center gap-1.5 rounded-md px-2 text-left hover:bg-muted aria-expanded:bg-muted pointer-coarse:min-h-touch';
</script>

{#snippet property(Icon: IconComponent, name: string, value: Snippet)}
  <div
    class="grid grid-cols-[6.5rem_minmax(0,1fr)] items-center @card-roomy/card:grid-cols-[9rem_minmax(0,1fr)]"
  >
    <dt class="flex items-center gap-2 text-muted-foreground">
      <Icon class="size-4 shrink-0" />
      <span class="truncate">{name}</span>
    </dt>
    <dd>{@render value()}</dd>
  </div>
{/snippet}

{#snippet empty()}
  <span class="text-muted-foreground">{T.empty}</span>
{/snippet}

{#snippet statusValue()}
  <Surface
    bind:open={statusOpen}
    {sheet}
    title={T.status}
    subtitle={sheet ? card.title : undefined}
    popoverClass="w-64"
  >
    {#snippet trigger(props: Record<string, unknown>)}
      <button {...props} type="button" data-testid="status-value" class={VALUE}>
        {#if statusOption}
          <Chip option={statusOption} />
        {:else}
          {@render empty()}
        {/if}
      </button>
    {/snippet}
    <SelectPicker
      options={STATUSES}
      selected={status}
      roomy={sheet}
      onselect={(id: string | undefined) => {
        status = id ?? '';
        statusOpen = false;
      }}
    />
  </Surface>
{/snippet}

{#snippet assigneeValue()}
  <button type="button" class={VALUE}>
    {#if assignee}
      <Avatar person={assignee} decorative />
      {assignee.name}
    {:else}
      {@render empty()}
    {/if}
  </button>
{/snippet}

{#snippet dueValue()}
  <Surface
    bind:open={dueOpen}
    {sheet}
    title={T.due}
    subtitle={sheet ? card.title : undefined}
    popoverClass="w-auto"
  >
    {#snippet trigger(props: Record<string, unknown>)}
      <button {...props} type="button" data-testid="due-value" class={VALUE}>
        {#if card.due}
          {card.due}
        {:else}
          {@render empty()}
        {/if}
      </button>
    {/snippet}
    <DatePicker />
  </Surface>
{/snippet}

{#snippet priorityValue()}
  <button type="button" class={VALUE}>
    {#if priority}
      <Chip option={priority} />
    {:else}
      {@render empty()}
    {/if}
  </button>
{/snippet}

{#snippet tagsValue()}
  <button type="button" class={[VALUE, 'flex-wrap py-1']}>
    {#each card.tags as tag (tag)}
      {@const option = optionOf(TAGS, tag)}
      {#if option}
        <Chip {option} />
      {/if}
    {:else}
      {@render empty()}
    {/each}
  </button>
{/snippet}

{#snippet hoursValue()}
  <button type="button" class={VALUE}>
    {#if card.hours !== undefined}
      {T.hoursOf(card.hours)}
    {:else}
      {@render empty()}
    {/if}
  </button>
{/snippet}

<article class="mx-auto flex w-full max-w-3xl flex-col gap-6 px-4 pt-2 pb-6 @card-roomy/card:px-8">
  <h1 data-testid="card-title" class="text-xl font-semibold @card-roomy/card:text-2xl">
    {card.title}
  </h1>

  <div class="flex flex-col">
    <dl class="flex flex-col">
      {@render property(CircleDotIcon, T.status, statusValue)}
      {@render property(UserIcon, T.assignee, assigneeValue)}
      {@render property(CalendarIcon, T.due, dueValue)}
      {@render property(FlagIcon, T.priority, priorityValue)}
      {@render property(TagIcon, T.tags, tagsValue)}
      {@render property(ClockIcon, T.hours, hoursValue)}
    </dl>
    <Button variant="ghost" class="mt-1 self-start text-muted-foreground pointer-coarse:h-touch">
      <PlusIcon data-icon="inline-start" />
      {T.addProperty}
    </Button>
  </div>

  <section aria-label={T.body} class="flex flex-col gap-3 border-t pt-5 text-base">
    {#each BODY.paragraphs as paragraph (paragraph)}
      <p>{paragraph}</p>
    {/each}
    <ul class="flex flex-col">
      {#each BODY.checklist as item (item.text)}
        <li class="flex min-h-8 items-center gap-2 pointer-coarse:min-h-touch">
          {#if item.done}
            <SquareCheckIcon class="size-5 shrink-0" />
            <span class="text-muted-foreground line-through">{item.text}</span>
          {:else}
            <SquareIcon class="size-5 shrink-0 text-muted-foreground" />
            <span>{item.text}</span>
          {/if}
        </li>
      {/each}
    </ul>
  </section>

  <section aria-labelledby="attachments-heading" class="flex flex-col gap-2 border-t pt-5">
    <h2 id="attachments-heading" class="font-semibold">
      {T.attachments} <span class="font-normal text-muted-foreground">{ATTACHMENTS.length}</span>
    </h2>
    <ul class="flex flex-col gap-2">
      {#each ATTACHMENTS as file (file.name)}
        <li class="flex items-center gap-3 rounded-lg border py-1 pr-1 pl-3">
          <FileTextIcon class="size-5 shrink-0 text-muted-foreground" />
          <div class="min-w-0 flex-1">
            <p class="truncate">{file.name}</p>
            <p class="text-xs text-muted-foreground">{file.size}</p>
          </div>
          <IconButton label={T.attachmentMenu(file.name)}><EllipsisIcon /></IconButton>
        </li>
      {/each}
    </ul>
  </section>

  <section aria-labelledby="comments-heading" class="flex flex-col gap-4 border-t pt-5">
    <h2 id="comments-heading" class="font-semibold">
      {T.comments} <span class="font-normal text-muted-foreground">{COMMENTS.length}</span>
    </h2>
    <ul class="flex flex-col gap-4">
      {#each COMMENTS as comment (comment.when)}
        {@const author = personOf(comment.author)}
        <li class="flex gap-3">
          {#if author}
            <Avatar person={author} decorative class="mt-0.5 size-7" />
          {/if}
          <div class="min-w-0 flex-1">
            <p class="flex flex-wrap items-baseline gap-x-2">
              <span class="font-medium">{author?.name}</span>
              <span class="text-xs text-muted-foreground">{comment.when}</span>
            </p>
            <p class="text-base">{comment.text}</p>
          </div>
        </li>
      {/each}
    </ul>
  </section>
</article>
