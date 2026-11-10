<!--
  The views of a board behind one button, where the header is folded: the
  button names the view that is shown, and opens the list of all of them (a
  sheet on a compact page). Picking one closes the list. A reader who may
  add views finds the kinds to add under the list.
-->
<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';

  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardView } from '$lib/logic/wire.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { ADD_VIEW_LABELS, VIEW_ICONS, viewTitle } from './texts.ts';
  import { ADDABLE_VIEW_TYPES, type AddableViewType } from './view-fields.ts';

  interface Props {
    /** In the order of the tabs. */
    views: readonly BoardView[];
    current: BoardView;
    /** The board, named under the title of the sheet. */
    boardTitle?: string;
    onOpenView: (viewId: string) => void;
    /** Without it the list only switches: the reader may not add views. */
    onAddView?: (viewType: AddableViewType) => void;
  }

  let { views, current, boardTitle, onOpenView, onAddView }: Props = $props();

  let open = $state(false);

  const CurrentIcon = $derived(VIEW_ICONS[current.fields.viewType]);

  const actions = $derived<MenuAction[]>([
    ...views.map((view) => ({
      id: view.id,
      label: viewTitle(view),
      icon: VIEW_ICONS[view.fields.viewType],
      checked: view.id === current.id,
      run: () => {
        if (view.id !== current.id) onOpenView(view.id);
      },
    })),
    ...(onAddView
      ? ADDABLE_VIEW_TYPES.map((viewType, at) => ({
          id: `add-${viewType}`,
          label: ADD_VIEW_LABELS[viewType](),
          icon: VIEW_ICONS[viewType],
          separated: at === 0,
          run: () => {
            onAddView(viewType);
          },
        }))
      : []),
  ]);
</script>

<ActionMenu bind:open title={m.viewheader_views()} subtitle={boardTitle} {actions} align="start">
  {#snippet trigger(props: Record<string, unknown>)}
    <Button
      {...props}
      variant="ghost"
      data-tool="views"
      class="min-w-0 shrink justify-start roomy:h-touch roomy:text-base"
    >
      <CurrentIcon data-icon="inline-start" />
      <span class="truncate">{viewTitle(current)}</span>
      <ChevronDownIcon data-icon="inline-end" class="text-muted-foreground" />
    </Button>
  {/snippet}
</ActionMenu>
