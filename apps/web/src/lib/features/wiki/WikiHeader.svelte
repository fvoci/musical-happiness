<!--
  The head of a wiki's tree: its icon, its name and its description; for its
  editors the way to its deleted pages, and for whoever may manage the wiki a
  menu to edit those and to delete the wiki.
-->
<script lang="ts">
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { WikiStore } from '$lib/state/index.ts';

  interface Props {
    wiki: WikiStore;
    onEdit: () => void;
    onDelete: () => void;
    /** Opens the wiki's deleted pages (its editors'). */
    onTrash: () => void;
  }

  let { wiki, onEdit, onDelete, onTrash }: Props = $props();

  const title = $derived(wiki.wiki?.title || m.wiki_untitled());
  const actions = $derived<MenuAction[]>([
    { id: 'edit-wiki', label: m.wiki_settings_title(), icon: PencilIcon, run: onEdit },
    {
      id: 'delete-wiki',
      label: m.shell_delete_wiki(),
      icon: Trash2Icon,
      destructive: true,
      separated: true,
      run: onDelete,
    },
  ]);
</script>

<div class="flex shrink-0 items-start gap-2 border-b px-3 py-3" data-testid="wiki-header">
  {#if wiki.wiki?.icon}
    <span class="text-xl leading-7" aria-hidden="true" data-testid="wiki-header-icon"
      >{wiki.wiki.icon}</span
    >
  {/if}
  <div class="min-w-0 flex-1">
    <p class="truncate leading-7 font-semibold" data-testid="wiki-header-title">{title}</p>
    {#if wiki.wiki?.description}
      <p
        class="line-clamp-3 text-sm whitespace-pre-line text-muted-foreground"
        data-testid="wiki-header-description"
      >
        {wiki.wiki.description}
      </p>
    {/if}
  </div>
  {#if wiki.canEdit}
    <IconButton
      label={m.wiki_trash()}
      data-testid="open-page-trash"
      aria-haspopup="dialog"
      class="text-muted-foreground"
      onclick={onTrash}
    >
      <Trash2Icon />
    </IconButton>
  {/if}
  {#if wiki.canManage}
    <ActionMenu {title} align="end" {actions}>
      {#snippet trigger(props: Record<string, unknown>)}
        <IconButton
          {...props}
          label={m.wiki_menu({ title })}
          data-testid="wiki-menu"
          class="text-muted-foreground"
        >
          <EllipsisIcon />
        </IconButton>
      {/snippet}
    </ActionMenu>
  {/if}
</div>
