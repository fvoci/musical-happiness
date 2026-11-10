<!--
  The frame a card opens in. Two proposals (`frame`), each with a narrow and a
  wide form:

  - `page`: on a narrow screen the card takes the whole screen, with a back
    arrow at the top left, like a page of its own. On a wide one it is a
    dialog in the middle, over the dimmed board.
  - `sheet`: on a narrow screen a sheet that rises to nearly the full height;
    a strip of the board stays visible above it. On a wide one a panel at the
    right edge, with the board still readable next to it.

  In all four the field for a new comment stays at the bottom edge, clear of
  the home indicator, and the back button closes the card.
-->
<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LayoutTemplateIcon from '@lucide/svelte/icons/layout-template';
  import LinkIcon from '@lucide/svelte/icons/link';
  import SendIcon from '@lucide/svelte/icons/send';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import XIcon from '@lucide/svelte/icons/x';

  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import * as Sheet from '$lib/ui/sheet/index.js';

  import ActionMenu from './ActionMenu.svelte';
  import type { Action } from './actions.ts';
  import Avatar from './Avatar.svelte';
  import BoardIcon from './BoardIcon.svelte';
  import CardBody from './CardBody.svelte';
  import { BOARD, ME, statusOf, type Card } from './data.ts';
  import { closeOnBack } from './history.svelte.ts';
  import IconButton from './IconButton.svelte';
  import { narrow } from './layout.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    open: boolean;
    card: Card;
    frame: 'page' | 'sheet';
    /** The property whose editor is open when the page loads. */
    editing: 'status' | 'due' | undefined;
    /** Menus and editors are bottom sheets. */
    sheet: boolean;
  }

  let { open = $bindable(), card, frame, editing, sheet }: Props = $props();

  let menuOpen = $state(false);
  let content = $state<HTMLElement | null>(null);

  const ACTIONS: Action[] = [
    { id: 'duplicate', label: T.duplicate, icon: CopyIcon },
    { id: 'link', label: T.copyLink, icon: LinkIcon },
    { id: 'template', label: T.saveAsTemplate, icon: LayoutTemplateIcon },
    { id: 'delete', label: T.deleteCard, icon: TrashIcon, destructive: true, separated: true },
  ];

  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );

  function close(): void {
    open = false;
  }

  /** The frame itself takes the focus: no ring on the first button, no keyboard. */
  function focusFrame(event: Event): void {
    event.preventDefault();
    content?.focus();
  }
</script>

{#snippet bar(leading: 'back' | 'close')}
  <div class="flex h-12 shrink-0 items-center gap-1 px-1.5 md:px-3">
    {#if leading === 'back'}
      <IconButton label={T.back} onclick={close}><ArrowLeftIcon /></IconButton>
    {/if}
    <p class="flex min-w-0 flex-1 items-center gap-1.5 px-1.5 text-sm text-muted-foreground">
      <BoardIcon board={BOARD} />
      <span class="truncate">{BOARD.name}</span>
      <ChevronRightIcon class="size-4 shrink-0" />
      <span class="shrink-0">{statusOf(card).label}</span>
    </p>
    <ActionMenu
      bind:open={menuOpen}
      {sheet}
      title={T.cardMenu}
      subtitle={card.title}
      actions={ACTIONS}
    >
      {#snippet trigger(props: Record<string, unknown>)}
        <IconButton {...props} label={T.cardMenu}><EllipsisIcon /></IconButton>
      {/snippet}
    </ActionMenu>
    {#if leading === 'close'}
      <IconButton label={T.close} onclick={close}><XIcon /></IconButton>
    {/if}
  </div>
{/snippet}

{#snippet body()}
  <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
    <CardBody {card} {editing} {sheet} />
  </div>
  <form
    class="flex shrink-0 items-center gap-2 border-t bg-background px-3 pt-2 pb-[calc(0.5rem+var(--safe-bottom))] md:px-6"
    onsubmit={(event) => {
      event.preventDefault();
    }}
  >
    <Avatar person={ME} decorative class="size-7" />
    <Input
      aria-label={T.writeComment}
      placeholder={T.writeComment}
      enterkeyhint="send"
      class="flex-1 pointer-coarse:h-touch"
    />
    <IconButton type="submit" label={T.send}><SendIcon /></IconButton>
  </form>
{/snippet}

{#if frame === 'page' && narrow.current}
  {#if open}
    <section
      data-testid="card-page"
      aria-label={card.title}
      class="fixed inset-0 z-40 flex flex-col bg-background pt-(--safe-top)"
    >
      {@render bar('back')}
      {@render body()}
    </section>
  {/if}
{:else if frame === 'page'}
  <Dialog.Root bind:open>
    <Dialog.Content
      bind:ref={content}
      showCloseButton={false}
      data-testid="card-dialog"
      class="flex h-[calc(100dvh-4rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl"
      onOpenAutoFocus={focusFrame}
    >
      <Dialog.Title class="sr-only">{card.title}</Dialog.Title>
      {@render bar('close')}
      {@render body()}
    </Dialog.Content>
  </Dialog.Root>
{:else}
  <Sheet.Root bind:open>
    <Sheet.Content
      bind:ref={content}
      side={narrow.current ? 'bottom' : 'right'}
      showCloseButton={false}
      data-testid="card-sheet"
      class="gap-0 outline-none data-[side=bottom]:h-[calc(100dvh-var(--safe-top)-2.5rem)] data-[side=bottom]:rounded-t-2xl data-[side=right]:w-[40rem] data-[side=right]:sm:max-w-none"
      onOpenAutoFocus={focusFrame}
    >
      {#if narrow.current}
        <div
          aria-hidden="true"
          class="mx-auto mt-2 h-1 w-9 shrink-0 rounded-full bg-input-border"
        ></div>
      {/if}
      <Sheet.Title class="sr-only">{card.title}</Sheet.Title>
      {@render bar('close')}
      {@render body()}
    </Sheet.Content>
  </Sheet.Root>
{/if}
