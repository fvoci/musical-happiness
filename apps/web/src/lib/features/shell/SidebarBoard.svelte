<!--
  One board in the sidebar: a link to it, and next to the link the button of
  its menu.

  The link itself is what is dragged (`data-drag-item`): a press on a control
  inside an item is the control's own, so the link cannot sit inside the item,
  and the menu button stands beside the link, not in it. A plain click opens
  the board in place; a click with a modifier, or the middle button, is left
  to the browser (a new tab).

  The menu button shows when the row is pointed at or holds the focus; on a
  device that cannot point without pressing it is always there.
-->
<script lang="ts">
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';

  import BoardIcon from './BoardIcon.svelte';

  interface Props {
    boardId: string;
    /** The title as shown: the board's own, or the words for a board without one. */
    title: string;
    icon: string | undefined;
    href: string;
    /** This is the board on the page. */
    current: boolean;
    locale: string;
    /** The board can be picked up now. Not while the list is narrowed by a search. */
    draggable: boolean;
    /** On its way: kept in the document, out of sight. */
    lifted: boolean;
    /** The element that explains dragging, for a screen reader. */
    describedBy: string;
    actions: readonly MenuAction[];
    onOpen: () => void;
  }

  let {
    boardId,
    title,
    icon,
    href,
    current,
    locale,
    draggable,
    lifted,
    describedBy,
    actions,
    onOpen,
  }: Props = $props();

  function onclick(event: MouseEvent): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    onOpen();
  }
</script>

<li class={['group/board relative', lifted && 'hidden']} data-board={boardId}>
  <a
    {href}
    draggable="false"
    data-drag-item={draggable ? boardId : undefined}
    aria-current={current ? 'page' : undefined}
    aria-describedby={draggable ? describedBy : undefined}
    class="flex min-h-8 items-center gap-2 rounded-md py-1 pr-9 pl-2 select-none [-webkit-touch-callout:none] hover:bg-sidebar-accent aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium roomy:min-h-touch roomy:pr-12"
    {onclick}
  >
    <BoardIcon {icon} {title} {locale} />
    <span class="min-w-0 flex-1 truncate">{title}</span>
  </a>
  <ActionMenu {title} {actions}>
    {#snippet trigger(props: Record<string, unknown>)}
      <IconButton
        {...props}
        label={m.shell_board_menu({ name: title })}
        class="absolute top-0 right-0 text-muted-foreground can-hover:opacity-0 can-hover:group-focus-within/board:opacity-100 can-hover:group-hover/board:opacity-100 can-hover:aria-expanded:opacity-100"
      >
        <EllipsisIcon />
      </IconButton>
    {/snippet}
  </ActionMenu>
</li>
