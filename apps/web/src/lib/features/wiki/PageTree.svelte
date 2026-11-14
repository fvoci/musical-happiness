<!--
  The pages of a wiki as a tree (`role="tree"`), and making pages: one more
  at the top of the wiki, one below a page, one right after a page.

  One page of the tree is in the tab order at a time (the page shown, or the
  last one moved to); the keys go as a tree's do: up and down to the page
  above and below, right to open a page or go to its first child, left to fold
  it or go to its parent, Home and End to the first and the last, Enter to
  show the page. The menu of a page opens from its button, or from the
  keyboard with Shift+F10 or the menu key. A page's children show while it is
  open; what is open is the reader's (`tree-open.svelte.ts`).
-->
<script lang="ts">
  import { tick } from 'svelte';

  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import FilePlusIcon from '@lucide/svelte/icons/file-plus';
  import ListPlusIcon from '@lucide/svelte/icons/list-plus';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { pageRows, type PageRow } from '$lib/logic/page-tree.ts';
  import type { PageDraft, WikiPage, WikiStore } from '$lib/state/index.ts';

  import type { TreeOpen } from './tree-open.svelte.ts';

  interface Props {
    wiki: WikiStore;
    /** The page shown. */
    currentId: string | undefined;
    open: TreeOpen;
    pageHref: (pageId: string) => string;
    onOpenPage: (pageId: string) => void;
    /** Makes a page as the draft says, and shows it. */
    onCreate: (draft: PageDraft) => void;
    /** Asks to delete a page; left out where pages are not deleted here. */
    onDelete?: ((page: WikiPage) => void) | undefined;
    /** A page is being made: no other is asked for meanwhile. */
    busy?: boolean;
  }

  let {
    wiki,
    currentId,
    open,
    pageHref,
    onOpenPage,
    onCreate,
    onDelete,
    busy = false,
  }: Props = $props();

  const uid = $props.id();
  const rows = $derived(pageRows(wiki.tree, (pageId) => open.has(pageId)));
  /**
   * The page that holds the tree's place in the tab order: the page shown,
   * or the one the reader moved to since it was shown.
   */
  let moved = $derived<string | undefined>(currentId);
  const active = $derived(
    [moved, currentId, rows[0]?.page.id].find(
      (pageId) => pageId !== undefined && rows.some((row) => row.page.id === pageId),
    ),
  );
  /** The page whose menu is open. */
  let menuOf = $state<string | undefined>(undefined);

  const titleOf = (page: WikiPage): string => page.title || m.wiki_page_untitled();
  const rowId = (pageId: string): string => `${uid}-${pageId}`;

  async function focusOn(pageId: string | undefined): Promise<void> {
    if (pageId === undefined) return;
    moved = pageId;
    await tick();
    document.getElementById(rowId(pageId))?.focus();
  }

  function onkeydown(event: KeyboardEvent): void {
    // The keys of the tree are its items'; a button inside an item keeps its own.
    if (event.defaultPrevented || !(event.target instanceof HTMLElement)) return;
    if (event.target.getAttribute('role') !== 'treeitem') return;
    const at = rows.findIndex((row) => row.page.id === active);
    const row = rows[at];
    if (!row || event.altKey || event.ctrlKey || event.metaKey) return;
    const step = (to: PageRow<WikiPage> | undefined): void => {
      event.preventDefault();
      void focusOn(to?.page.id);
    };
    if (wiki.canEdit && (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))) {
      event.preventDefault();
      menuOf = row.page.id;
      return;
    }
    if (event.shiftKey) return;
    switch (event.key) {
      case 'ArrowDown':
        step(rows[at + 1]);
        break;
      case 'ArrowUp':
        step(rows[at - 1]);
        break;
      case 'Home':
        step(rows[0]);
        break;
      case 'End':
        step(rows.at(-1));
        break;
      case 'ArrowRight':
        if (!row.hasChildren) break;
        if (row.open) step(rows[at + 1]);
        else {
          event.preventDefault();
          open.set([row.page.id]);
        }
        break;
      case 'ArrowLeft':
        if (row.open) {
          event.preventDefault();
          open.set([row.page.id], false);
        } else step(rows.find((one) => one.page.id === row.parentId));
        break;
      case 'Enter':
        event.preventDefault();
        onOpenPage(row.page.id);
        break;
      default:
        break;
    }
  }

  function onclick(event: MouseEvent, pageId: string): void {
    moved = pageId;
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    onOpenPage(pageId);
  }

  function actionsOf(row: PageRow<WikiPage>): MenuAction[] {
    const actions: MenuAction[] = [
      {
        id: 'add-child',
        label: m.wiki_add_child(),
        icon: ListPlusIcon,
        run: () => {
          open.set([row.page.id]);
          onCreate({ parentId: row.page.id });
        },
      },
      {
        id: 'add-after',
        label: m.wiki_add_after(),
        icon: FilePlusIcon,
        run: () => {
          onCreate({ parentId: row.parentId ?? '', after: row.page.id });
        },
      },
    ];
    if (onDelete) {
      const remove = onDelete;
      actions.push({
        id: 'delete',
        label: m.wiki_delete_page(),
        icon: Trash2Icon,
        destructive: true,
        separated: true,
        run: () => {
          remove(row.page);
        },
      });
    }
    return actions;
  }
</script>

<div class="flex min-h-0 flex-1 flex-col" data-testid="page-tree">
  <div class="flex shrink-0 items-center gap-1 px-3 pt-3 pb-1">
    <h2 id="{uid}-title" class="min-w-0 flex-1 truncate text-xs font-medium text-muted-foreground">
      {m.wiki_pages()}
    </h2>
    {#if wiki.canEdit}
      <IconButton
        label={m.wiki_new_page()}
        data-testid="new-page"
        disabled={busy}
        class="text-muted-foreground"
        onclick={() => {
          onCreate({});
        }}
      >
        <PlusIcon />
      </IconButton>
    {/if}
  </div>
  {#if rows.length > 0}
    <ul
      role="tree"
      aria-labelledby="{uid}-title"
      class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-3"
      {onkeydown}
    >
      {#each rows as row (row.page.id)}
        {@const page = row.page}
        {@const title = titleOf(page)}
        <li
          id={rowId(page.id)}
          role="treeitem"
          aria-label={title}
          aria-level={row.depth + 1}
          aria-setsize={row.siblings}
          aria-posinset={row.position}
          aria-expanded={row.hasChildren ? row.open : undefined}
          aria-selected={page.id === currentId}
          tabindex={page.id === active ? 0 : -1}
          data-page={page.id}
          class="group/page relative rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onfocus={(event: FocusEvent) => {
            if (event.target === event.currentTarget) moved = page.id;
          }}
        >
          <div
            class={[
              'flex min-h-8 items-center gap-1 rounded-md pr-9 hover:bg-muted roomy:min-h-touch roomy:pr-12',
              page.id === currentId && 'bg-muted font-medium',
            ]}
            style:padding-left="{0.25 + row.depth * 1}rem"
          >
            {#if row.hasChildren}
              <button
                type="button"
                tabindex="-1"
                aria-label={row.open ? m.wiki_fold({ title }) : m.wiki_unfold({ title })}
                class="grid size-6 shrink-0 place-items-center rounded-sm text-muted-foreground hover:bg-accent roomy:size-9"
                onclick={() => {
                  open.set([page.id], !row.open);
                }}
              >
                <ChevronRightIcon
                  class={['size-4 transition-transform', row.open && 'rotate-90']}
                  aria-hidden="true"
                />
              </button>
            {:else}
              <span class="size-6 shrink-0 roomy:size-9" aria-hidden="true"></span>
            {/if}
            <a
              href={pageHref(page.id)}
              tabindex="-1"
              draggable="false"
              class="flex min-w-0 flex-1 items-center gap-1.5 py-1"
              onclick={(event: MouseEvent) => {
                onclick(event, page.id);
              }}
            >
              {#if page.icon}
                <span class="shrink-0" aria-hidden="true" data-testid="tree-icon">{page.icon}</span>
              {/if}
              <span class={['min-w-0 truncate', !page.title && 'text-muted-foreground']}>
                {title}
              </span>
            </a>
          </div>
          {#if wiki.canEdit}
            <ActionMenu
              bind:open={
                () => menuOf === page.id,
                (shown: boolean) => {
                  menuOf = shown ? page.id : undefined;
                  // Closed, the menu gives the focus back to its page, where the keys of the tree are.
                  if (!shown) void focusOn(page.id);
                }
              }
              {title}
              actions={actionsOf(row)}
            >
              {#snippet trigger(props: Record<string, unknown>)}
                <IconButton
                  {...props}
                  tabindex={-1}
                  label={m.wiki_page_menu({ title })}
                  class="absolute top-0 right-0 text-muted-foreground can-hover:opacity-0 can-hover:group-focus-within/page:opacity-100 can-hover:group-hover/page:opacity-100 can-hover:aria-expanded:opacity-100"
                >
                  <EllipsisIcon />
                </IconButton>
              {/snippet}
            </ActionMenu>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
