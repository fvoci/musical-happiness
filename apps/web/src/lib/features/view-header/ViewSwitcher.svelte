<!--
  The views of a board behind one button, at every width of the header. The
  button names the view that is shown (the icon of its kind and its name,
  cut short where it is long and whole in its tooltip) and opens the list of
  all of them, in their order, the one shown marked: a dropdown, or a sheet
  on a compact page. Each line is a link to its view's address, which a
  click that asks for a new tab or window opens there; any other click, and
  Enter, shows the view here, and the focus is back on the button, which
  now names it.

  A reader who may change the board finds under the views the kinds of view
  to add, and what is done to the view that is shown: a new name, a copy,
  deleting it. The name is changed in a small surface that opens where the
  button is (a popover, or a sheet on a compact page): Enter, "Save" and
  whatever else closes the surface save it, Escape leaves it as it was. The
  text being typed is kept here, above the surface, so that it outlives a
  surface that is rebuilt when the sheet becomes a popover. A board has at
  least one view: the last one cannot be deleted. Deleting asks first, and
  the page shows another view once this one is gone.
-->
<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { tick } from 'svelte';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isComposing } from '$lib/keys.ts';
  import type { BoardView } from '$lib/logic/wire.ts';
  import { whileHere, type BoardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';

  import { ADD_VIEW_LABELS, NEW_VIEW_TITLES, VIEW_ICONS, viewTitle } from './texts.ts';
  import { ADDABLE_VIEW_TYPES, newViewFields, type AddableViewType } from './view-fields.ts';

  interface Props {
    board: BoardStore;
    /** The view that is shown. */
    view: BoardView;
    viewHref: (viewId: string) => string;
    onOpenView: (viewId: string) => void;
  }

  let { board, view, viewHref, onOpenView }: Props = $props();

  const pickers = new PickerPresentation();
  const CurrentIcon = $derived(VIEW_ICONS[view.fields.viewType]);

  let renameOpen = $state(false);
  let deleteOpen = $state(false);
  let box = $state<HTMLElement | null>(null);

  let renameDraft = $state('');
  /** The view the name is for: the page may show another by the time the surface closes. */
  let renaming: string | undefined;
  /** Escape in the field: the name stays as it was. Not reactive, it is read once on closing. */
  let renameCancelled = false;

  function setRenameOpen(open: boolean): void {
    if (open === renameOpen) return;
    if (open) {
      renaming = view.id;
      renameDraft = view.title;
      renameCancelled = false;
    } else {
      const title = renameDraft.trim();
      const named = board.view(renaming);
      if (!renameCancelled && named && title !== named.title) {
        void board.changeView(named.id, (current) => ({ ...current, title }), 'renameView');
      }
      // The element the surface opened at cannot take the focus back; the button does.
      void tick().then(() => box?.querySelector<HTMLElement>('[data-tool="views"]')?.focus());
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

  async function addView(viewType: AddableViewType): Promise<void> {
    const fields = newViewFields(viewType, board.board?.cardProperties ?? []);
    const title = NEW_VIEW_TITLES[viewType]();
    const made = await whileHere(board, () => board.createView({ title, fields }));
    if (made) onOpenView(made.id);
  }

  async function duplicate(): Promise<void> {
    const title = m.viewheader_copy_title({ title: viewTitle(view) });
    const copy = await whileHere(board, () => board.duplicateView(view.id, title));
    if (copy) onOpenView(copy.id);
  }

  const viewLines = $derived<MenuAction[]>(
    board.orderedViews.map((each) => ({
      id: each.id,
      label: viewTitle(each),
      icon: VIEW_ICONS[each.fields.viewType],
      checked: each.id === view.id,
      href: viewHref(each.id),
      run: () => {
        if (each.id !== view.id) onOpenView(each.id);
      },
    })),
  );

  /** What a reader who may change the board does here: add a view, and change the one shown. */
  const editLines = $derived<MenuAction[]>([
    ...ADDABLE_VIEW_TYPES.map((viewType, at) => ({
      id: `add-${viewType}`,
      label: ADD_VIEW_LABELS[viewType](),
      icon: VIEW_ICONS[viewType],
      separated: at === 0,
      run: () => void addView(viewType),
    })),
    {
      id: 'rename',
      label: m.viewheader_rename_view(),
      icon: PencilIcon,
      separated: true,
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

<span bind:this={box} class="relative inline-flex max-w-80 min-w-0 shrink">
  {#if board.canEdit}
    <PickerSurface
      bind:open={() => renameOpen, setRenameOpen}
      presentation={pickers.current}
      title={m.viewheader_rename_view()}
    >
      {#snippet trigger({ props }: { props: Record<string, unknown> })}
        <!-- Nothing to press: the surface is opened from the list, and opens where the button is. -->
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
  {/if}
  <ActionMenu
    title={m.viewheader_views()}
    subtitle={board.board?.title}
    actions={board.canEdit ? [...viewLines, ...editLines] : viewLines}
    align="start"
  >
    {#snippet trigger(props: Record<string, unknown>)}
      <Button
        {...props}
        variant="ghost"
        data-tool="views"
        title={viewTitle(view)}
        class="min-w-0 shrink justify-start roomy:h-touch roomy:text-base"
      >
        <CurrentIcon data-icon="inline-start" />
        <span class="truncate">{viewTitle(view)}</span>
        <ChevronDownIcon data-icon="inline-end" class="text-muted-foreground" />
      </Button>
    {/snippet}
  </ActionMenu>
</span>

{#if board.canEdit}
  <ConfirmDialog
    bind:open={deleteOpen}
    title={m.viewheader_delete_view_title({ name: viewTitle(view) })}
    description={m.viewheader_delete_view_description()}
    confirmLabel={m.shared_delete()}
    destructive
    onConfirm={() => void board.deleteView(view.id)}
    focusAfter={() =>
      // The line it was asked from is gone with the list: the button, which names the view now.
      box?.querySelector<HTMLElement>('[data-tool="views"]')}
  />
{/if}
