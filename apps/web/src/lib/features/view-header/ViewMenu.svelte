<!--
  "…" of the header: what is done to the view itself (a new name, a copy,
  deleting it), below whatever lines the header adds for the tools that have
  no button of their own in its folded row.

  The name is changed in a small surface that opens where "…" is: a popover,
  or a sheet on a compact page. Enter, "Save" and whatever else closes the
  surface save the name; Escape leaves it as it was. The text being typed is
  kept here, above the surface, so that it outlives a surface that is rebuilt
  when the sheet becomes a popover.

  A board has at least one view: the last one cannot be deleted. Deleting
  asks first. The page shows another view once this one is gone.
-->
<script lang="ts">
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { tick } from 'svelte';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { isComposing } from '$lib/keys.ts';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardView } from '$lib/logic/wire.ts';
  import { whileHere, type BoardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';

  import { viewTitle } from './texts.ts';

  interface Props {
    board: BoardStore;
    view: BoardView;
    /** Lines above the view's own: the tools the header has no button for. */
    tools: readonly MenuAction[];
    onOpenView: (viewId: string) => void;
  }

  let { board, view, tools, onOpenView }: Props = $props();

  const pickers = new PickerPresentation();

  let menuOpen = $state(false);
  let renameOpen = $state(false);
  let deleteOpen = $state(false);
  let button = $state<HTMLElement | null>(null);

  let renameDraft = $state('');
  /** Escape in the field: the name stays as it was. Not reactive, it is read once on closing. */
  let renameCancelled = false;

  function setRenameOpen(open: boolean): void {
    if (open === renameOpen) return;
    if (open) {
      renameDraft = view.title;
      renameCancelled = false;
    } else {
      const title = renameDraft.trim();
      if (!renameCancelled && title !== view.title) {
        void board.changeView(view.id, (current) => ({ ...current, title }), 'renameView');
      }
      // The element the surface opened at cannot take the focus back; "…" does.
      void tick().then(() => button?.querySelector<HTMLElement>('[data-tool="more"]')?.focus());
    }
    renameOpen = open;
  }

  function renameKeys(event: KeyboardEvent): void {
    if (isComposing(event)) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      setRenameOpen(false);
    } else if (event.key === 'Escape') {
      // Whatever closes the surface next finds the mark and saves nothing.
      renameCancelled = true;
    }
  }

  async function duplicate(): Promise<void> {
    const title = m.viewheader_copy_title({ title: viewTitle(view) });
    const copy = await whileHere(board, () => board.duplicateView(view.id, title));
    if (copy) onOpenView(copy.id);
  }

  const actions = $derived<MenuAction[]>([
    ...tools,
    {
      id: 'rename',
      label: m.viewheader_rename_view(),
      icon: PencilIcon,
      separated: tools.length > 0,
      run: () => {
        setRenameOpen(true);
      },
    },
    {
      id: 'duplicate',
      label: m['View.DuplicateView'](),
      icon: CopyIcon,
      run: () => void duplicate(),
    },
    {
      id: 'delete',
      label: m['View.DeleteView'](),
      icon: TrashIcon,
      destructive: true,
      separated: true,
      disabled: board.views.length <= 1,
      run: () => {
        deleteOpen = true;
      },
    },
  ]);
</script>

<span bind:this={button} class="relative inline-flex shrink-0">
  <PickerSurface
    bind:open={() => renameOpen, setRenameOpen}
    presentation={pickers.current}
    title={m.viewheader_rename_view()}
  >
    {#snippet trigger({ props }: { props: Record<string, unknown> })}
      <!-- Nothing to press: the surface is opened from the menu, and opens where "…" is. -->
      <button
        {...props}
        type="button"
        tabindex="-1"
        aria-hidden="true"
        data-tool="rename"
        class="invisible absolute inset-0"
      ></button>
    {/snippet}
    <div class="flex items-center gap-2 p-2" data-testid="rename-view">
      <Input
        bind:value={renameDraft}
        aria-label={m.viewheader_view_name()}
        placeholder={m['Sidebar.untitled-view']()}
        autocomplete="off"
        enterkeyhint="done"
        data-picker-focus
        class="min-w-48 roomy:h-touch roomy:text-base"
        onkeydown={renameKeys}
      />
      <Button
        class="shrink-0 roomy:h-touch"
        onclick={() => {
          setRenameOpen(false);
        }}
      >
        {m.viewheader_rename_save()}
      </Button>
    </div>
  </PickerSurface>
  <ActionMenu
    bind:open={menuOpen}
    title={m['ViewHeader.view-menu']()}
    subtitle={viewTitle(view)}
    {actions}
  >
    {#snippet trigger(props: Record<string, unknown>)}
      <IconButton {...props} label={m['ViewHeader.view-menu']()} data-tool="more">
        <EllipsisIcon />
      </IconButton>
    {/snippet}
  </ActionMenu>
</span>

<ConfirmDialog
  bind:open={deleteOpen}
  title={m.viewheader_delete_view_title({ name: viewTitle(view) })}
  description={m.viewheader_delete_view_description()}
  confirmLabel={m.shared_delete()}
  destructive
  onConfirm={() => void board.deleteView(view.id)}
/>
