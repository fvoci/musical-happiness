<!--
  The face of a card: its icon and title, the properties the view turned on,
  and what its body holds (that there is one, how its tasks stand). A kanban
  column and a gallery draw the same face, the gallery a larger one.

  This is only what a card looks like. What makes it a thing to press, drag
  or select is on the element around it. So is its menu: the caller lays the
  button over the top corner, and `menuRoom` keeps the title clear of it.
-->
<script lang="ts">
  import SquareCheckIcon from '@lucide/svelte/icons/square-check';
  import TextAlignStartIcon from '@lucide/svelte/icons/text-align-start';

  import { PropertyValue, type PropertyUser } from '$lib/components/properties/index.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Regional } from '$lib/logic/regional.ts';
  import type { Card, PropertyTemplate } from '$lib/logic/wire.ts';

  import { faceProperties } from './face.ts';

  interface Props {
    card: Card;
    /** The definitions of the board's card properties. */
    templates: readonly PropertyTemplate[];
    /** The properties the view shows on its cards, in its order. */
    visiblePropertyIds: readonly string[];
    /** The card has a body. */
    hasBody?: boolean;
    /** The tasks in the body: how many, and how many are done. */
    tasksTotal?: number;
    tasksDone?: number;
    /** `large`: the card of a gallery. */
    size?: 'small' | 'large';
    /** The id of the title, for the element around the face to be named by. */
    titleId?: string;
    locale: string;
    regional: Regional;
    users?: readonly PropertyUser[];
    nameFormat?: string;
    /** The current moment, for a test. */
    now?: number;
    /** Off the board: under the pointer, or carried by the keyboard. */
    lifted?: boolean;
    selected?: boolean;
    /** How many cards travel with this one, itself included. Shown when more than one. */
    count?: number;
    /** A menu button lies over the top corner at the end of the title. */
    menuRoom?: boolean;
    /** Classes for the box of the face, such as a height to fill. */
    class?: string;
  }

  let {
    card,
    templates,
    visiblePropertyIds,
    hasBody = false,
    tasksTotal = 0,
    tasksDone = 0,
    size = 'small',
    titleId,
    locale,
    regional,
    users = [],
    nameFormat = '',
    now,
    lifted = false,
    selected = false,
    count = 1,
    menuRoom = false,
    class: className,
  }: Props = $props();

  const shown = $derived(faceProperties(card, templates, visiblePropertyIds));
  const title = $derived(card.title || m['KanbanCard.untitled']());
  const large = $derived(size === 'large');
</script>

<div
  data-slot="card-face"
  data-size={size}
  class={[
    'relative rounded-md border bg-card text-card-foreground',
    large ? 'p-4' : 'p-3',
    lifted ? 'shadow-lg ring-2 ring-ring' : 'shadow-xs',
    selected && 'border-primary ring-2 ring-primary',
    className,
  ]}
>
  <!-- Room at the end of the title for the menu button: 32 pixels, 44 where a finger presses. -->
  <p
    id={titleId}
    data-slot="card-title"
    class={[
      'flex min-w-0 items-start gap-1.5 font-medium wrap-anywhere',
      large && 'text-base',
      menuRoom && 'pr-7 roomy:pr-9',
      !card.title && 'text-muted-foreground',
    ]}
  >
    {#if card.fields.icon}
      <span data-slot="card-icon" class="shrink-0" aria-hidden="true">{card.fields.icon}</span>
    {/if}
    <span data-slot="card-title-text" class="min-w-0">{title}</span>
  </p>

  {#if shown.length > 0}
    <!-- The kinds bring their own side padding; pulled out so that the values line up with the title. -->
    <div
      data-slot="card-properties"
      class={['-mx-2 flex flex-col text-sm', large ? 'mt-2' : 'mt-1']}
    >
      {#each shown as template (template.id)}
        <div data-property={template.id} title={template.name} class="flex min-w-0 items-center">
          <span class="sr-only">{template.name}:</span>
          <div class={template.type === 'checkbox' ? 'shrink-0' : 'min-w-0 flex-1'}>
            <PropertyValue
              {template}
              {card}
              value={card.fields.properties[template.id]}
              mode="display"
              {users}
              {nameFormat}
              {locale}
              {regional}
              {now}
            />
          </div>
          {#if template.type === 'checkbox'}
            <!-- A box alone says nothing: its property is named next to it. -->
            <span class="min-w-0 truncate text-muted-foreground" aria-hidden="true">
              {template.name}
            </span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if hasBody || tasksTotal > 0}
    <div
      data-slot="card-badges"
      class={['flex items-center gap-3 text-xs text-muted-foreground', large ? 'mt-3' : 'mt-2']}
    >
      {#if hasBody}
        <span data-slot="card-has-body" title={m['CardBadges.title-description']()}>
          <TextAlignStartIcon class="size-3.5" aria-hidden="true" />
          <span class="sr-only">{m['CardBadges.title-description']()}</span>
        </span>
      {/if}
      {#if tasksTotal > 0}
        <span
          data-slot="card-tasks"
          class="flex items-center gap-1"
          title={m['CardBadges.title-checkboxes']()}
        >
          <SquareCheckIcon class="size-3.5" aria-hidden="true" />
          <span class="sr-only">{m['CardBadges.title-checkboxes']()}</span>
          {tasksDone}/{tasksTotal}
        </span>
      {/if}
    </div>
  {/if}

  {#if count > 1}
    <span
      data-slot="card-count"
      class="absolute -end-2 -top-2 flex h-6 min-w-6 items-center justify-center rounded-full bg-primary px-1.5 text-xs font-semibold text-primary-foreground"
    >
      {count}
    </span>
  {/if}
</div>
