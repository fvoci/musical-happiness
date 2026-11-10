<!--
  The top row of an open card: how to leave it, where it belongs (the board
  and the view it was opened from), how the saving of its body stands, and
  its menu.

  Leaving is a back arrow at the left when the card fills the screen, as a
  page would have it, and a close button at the right when it is a panel or a
  dialog. Both do the same.
-->
<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LinkIcon from '@lucide/svelte/icons/link';
  import Redo2Icon from '@lucide/svelte/icons/redo-2';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import Undo2Icon from '@lucide/svelte/icons/undo-2';
  import XIcon from '@lucide/svelte/icons/x';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { CardFrame } from '$lib/layout/presentation.ts';
  import type { BoardView } from '$lib/logic/wire.ts';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';

  import type { SaveState } from './body-saver.svelte.ts';

  interface Props {
    board: BoardStore;
    view: BoardView | undefined;
    card: CardStore;
    frame: CardFrame;
    /** How the saving of the body stands. */
    saveState: SaveState;
    onClose: () => void;
    onDuplicate: () => void;
    onDelete: () => void;
  }

  let { board, view, card, frame, saveState, onClose, onDuplicate, onDelete }: Props = $props();

  let menuOpen = $state(false);
  let confirmingDelete = $state(false);
  /** Said once in place of the saving state: the link was copied. */
  let copied = $state(false);

  const workspace = $derived(board.workspace);
  const cardTitle = $derived(card.summary?.title || m['KanbanCard.untitled']());
  const there = $derived(card.summary !== undefined && !card.missing);

  const SAVE_TEXT: Record<SaveState, () => string> = {
    idle: () => '',
    saving: m.card_saving,
    saved: m.card_saved,
    unsaved: m.card_not_saved,
    conflict: m.card_not_saved,
  };

  async function copyLink(): Promise<void> {
    try {
      // A card is an address: the one in the address bar is the link to it.
      await navigator.clipboard.writeText(location.href);
    } catch {
      // No clipboard here (a page that is not served securely): nothing was copied, so nothing is said.
      return;
    }
    copied = true;
    setTimeout(() => {
      copied = false;
    }, 2000);
  }

  const actions = $derived.by<MenuAction[]>(() => {
    const list: MenuAction[] = [];
    if (board.canComment) {
      list.push(
        {
          id: 'undo',
          label: m['UndoRedoHotKeys.canUndo'](),
          icon: Undo2Icon,
          disabled: !workspace.canUndo,
          run: () => void workspace.undoLast(),
        },
        {
          id: 'redo',
          label: m['UndoRedoHotKeys.canRedo'](),
          icon: Redo2Icon,
          disabled: !workspace.canRedo,
          run: () => void workspace.redoLast(),
        },
      );
    }
    if (there && board.canEdit) {
      list.push({
        id: 'duplicate',
        label: m['CardActionsMenu.duplicate'](),
        icon: CopyIcon,
        separated: list.length > 0,
        run: onDuplicate,
      });
    }
    if (there) {
      list.push({
        id: 'copy-link',
        label: m['CardActionsMenu.copyLink'](),
        icon: LinkIcon,
        separated: list.length > 0 && !board.canEdit,
        run: () => void copyLink(),
      });
    }
    if (there && board.canEdit) {
      list.push({
        id: 'delete',
        label: m['CardActionsMenu.delete'](),
        icon: Trash2Icon,
        destructive: true,
        separated: true,
        run: () => {
          confirmingDelete = true;
        },
      });
    }
    return list;
  });
</script>

<header
  class="flex h-12 shrink-0 items-center gap-1 px-1.5 @card-roomy/card:px-3"
  data-testid="card-header"
>
  {#if frame === 'full'}
    <IconButton label={m.ui_back()} data-testid="card-back" onclick={onClose}>
      <ArrowLeftIcon aria-hidden="true" />
    </IconButton>
  {/if}
  <p
    class="flex min-w-0 flex-1 items-center gap-1.5 px-1.5 text-muted-foreground"
    data-testid="card-location"
  >
    {#if board.board?.icon}
      <span class="shrink-0" aria-hidden="true">{board.board.icon}</span>
    {/if}
    <span class="min-w-0 shrink truncate">
      {board.board?.title || m['Sidebar.untitled-board']()}
    </span>
    {#if view}
      <ChevronRightIcon class="size-4 shrink-0" aria-hidden="true" />
      <span class="min-w-0 shrink-[2] truncate">{view.title || m['Sidebar.untitled-view']()}</span>
    {/if}
  </p>
  <span
    class="shrink-0 px-1 text-xs text-muted-foreground"
    role="status"
    aria-live="polite"
    data-testid="card-save-state"
    data-state={saveState}
  >
    {copied ? m['CardActionsMenu.copiedLink']() : SAVE_TEXT[saveState]()}
  </span>
  {#if actions.length > 0}
    <ActionMenu bind:open={menuOpen} title={m.card_menu()} subtitle={cardTitle} {actions}>
      {#snippet trigger(props: Record<string, unknown>)}
        <IconButton {...props} label={m.card_menu()} data-testid="card-menu">
          <EllipsisIcon aria-hidden="true" />
        </IconButton>
      {/snippet}
    </ActionMenu>
  {/if}
  {#if frame !== 'full'}
    <IconButton label={m.ui_close()} data-testid="card-close" onclick={onClose}>
      <XIcon aria-hidden="true" />
    </IconButton>
  {/if}
</header>

<ConfirmDialog
  bind:open={confirmingDelete}
  title={m['CardDialog.delete-confirmation-dialog-heading']()}
  description={m.card_delete_description({ title: cardTitle })}
  confirmLabel={m['CardDialog.delete-confirmation-dialog-button-text']()}
  destructive
  onConfirm={onDelete}
/>
