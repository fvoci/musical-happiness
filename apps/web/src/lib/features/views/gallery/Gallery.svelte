<!-- A stand-in until the view is built: the cards as a plain list. -->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';

  import type { ViewProps } from '../props.ts';

  let { cards, cardHref, onOpenCard }: ViewProps = $props();
</script>

<ul class="h-full overflow-y-auto p-4" data-testid="view-stand-in">
  {#each cards as card (card.id)}
    <li>
      <a
        class="block rounded-md px-2 py-1.5 hover:bg-muted roomy:py-3"
        href={cardHref(card.id)}
        data-card={card.id}
        onclick={(event) => {
          event.preventDefault();
          onOpenCard(card.id);
        }}
      >
        {card.title || m['KanbanCard.untitled']()}
      </a>
    </li>
  {/each}
</ul>
