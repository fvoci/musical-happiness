<!--
  The "…" of a card in a list of the calendar (the cards of a day, the cards
  without a date). Its first line changes the card's date without a drag
  (question C9, answer B): "Change date", or "Set date" for a card that has
  none. It is missing where the calendar is placed by a date nobody can set
  (when cards were made or changed). Then duplicate and delete, as on a
  board; undo brings a deleted card back.

  Only for someone who may edit. Where the device can hover, the button waits
  for its line to be pointed at or focused; anywhere else it is always there.
  The line carries the class `group/card`.
-->
<script lang="ts">
  import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import TrashIcon from '@lucide/svelte/icons/trash-2';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Card } from '$lib/logic/wire.ts';
  import type { BoardStore } from '$lib/state/index.ts';

  interface Props {
    board: BoardStore;
    card: Card;
    title: string;
    /** The card has a date to change; else it is given one. */
    dated: boolean;
    /** Opens the picker of a day for the card; none where dates cannot be set. */
    onChangeDate?: (cardId: string) => void;
  }

  let { board, card, title, dated, onChangeDate }: Props = $props();

  let open = $state(false);

  const actions = $derived.by((): MenuAction[] => {
    const list: MenuAction[] = [];
    if (onChangeDate) {
      const change = onChangeDate;
      list.push({
        id: 'change-date',
        label: dated ? m.calendar_change_date() : m.calendar_set_date(),
        icon: CalendarClockIcon,
        // The menu closes, and gives the focus back, before the picker opens.
        run: () => {
          setTimeout(() => {
            change(card.id);
          }, 0);
        },
      });
    }
    list.push(
      {
        id: 'duplicate',
        label: m['CardActionsMenu.duplicate'](),
        icon: CopyIcon,
        run: () => void board.duplicateCard(card.id),
      },
      {
        id: 'delete',
        label: m['CardActionsMenu.delete'](),
        icon: TrashIcon,
        destructive: true,
        separated: true,
        run: () => void board.deleteCards([card.id]),
      },
    );
    return list;
  });
</script>

<ActionMenu bind:open title={m.kanban_card_menu({ title })} subtitle={title} {actions}>
  {#snippet trigger(props: Record<string, unknown>)}
    <IconButton
      {...props}
      label={m.kanban_card_menu({ title })}
      data-card-menu={card.id}
      class="shrink-0 text-muted-foreground can-hover:opacity-0 can-hover:group-focus-within/card:opacity-100 can-hover:group-hover/card:opacity-100 can-hover:focus-visible:opacity-100 can-hover:aria-expanded:opacity-100"
    >
      <EllipsisIcon aria-hidden="true" />
    </IconButton>
  {/snippet}
</ActionMenu>
