<!--
  The frame a card opens in. Two proposals (`frame`), each with a form for a
  compact, a medium and an expanded window:

  - `modal`: the card is over the board, and the board waits. A page that
    takes the whole screen, with a back arrow at the top left (compact); a
    panel that comes in from the right, the board dimmed beside it (medium);
    a dialog in the middle (expanded).
  - `beside`: a sheet that rises to nearly the full height, a strip of the
    board visible above it (compact); the same panel from the right (medium:
    the board would be left with too little); a pane next to the board, which
    stays in use and is laid out anew in the width that is left (expanded).
    The pane is half the page, so its edge is on the line a screen folds along.

  The three forms of `modal` are one dialog whose classes change. Nothing is
  put together again when the window changes size under an open card: what
  was typed, where it was scrolled to and where the caret is all stay. The
  forms of `beside` are different things, and the card is built again when
  the window crosses between them; the draft of a comment and the scroll
  position are carried over by hand, the focus is not.

  In every form the field for a new comment stays at the bottom edge, clear
  of the home indicator, and the back button closes the card.
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
  import { app, type Stage } from './layout.svelte.ts';
  import { T } from './text.ts';

  interface Props {
    open: boolean;
    card: Card;
    frame: 'modal' | 'beside';
    /** The property whose editor is open when the page loads. */
    editing: 'status' | 'due' | undefined;
    /** Menus and editors are bottom sheets. */
    sheet: boolean;
  }

  let { open = $bindable(), card, frame, editing, sheet }: Props = $props();

  let menuOpen = $state(false);
  let content = $state<HTMLElement | null>(null);
  /** The comment being written. Kept here so that it outlives a change of form. */
  let draft = $state('');
  /** How far the card was scrolled, for the same reason. */
  let scrolledTo = 0;

  const stage = $derived(app.stage);
  const form = $derived(
    frame === 'modal' || stage === 'medium' ? 'dialog' : stage === 'compact' ? 'sheet' : 'pane',
  );

  /** One dialog, three placements. Every class that differs is written out for each. */
  const DIALOG: Record<Stage, string> = {
    compact:
      'top-0 left-0 h-dvh w-full max-w-none translate-x-0 translate-y-0 rounded-none bg-background pt-(--safe-top) pr-(--safe-right) pl-(--safe-left) ring-0 sm:max-w-none',
    medium:
      'top-0 right-0 left-auto h-dvh w-[min(calc(30rem+var(--safe-right)),calc(100%-3.5rem))] max-w-none translate-x-0 translate-y-0 rounded-none pt-(--safe-top) pr-(--safe-right) ring-0 border-l shadow-lg sm:max-w-none',
    expanded:
      'left-[calc(50%+(var(--safe-left)-var(--safe-right))/2)] h-[calc(100dvh-4rem)] max-w-[min(48rem,calc(100%-2rem-var(--safe-left)-var(--safe-right)))] sm:max-w-[min(48rem,calc(100%-2rem-var(--safe-left)-var(--safe-right)))]',
  };
  const DIALOG_NAME: Record<Stage, string> = {
    compact: 'card-page',
    medium: 'card-panel',
    expanded: 'card-dialog',
  };

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

  /** Puts a newly built card where the one before it was scrolled to. */
  function keepScroll(element: HTMLElement): void {
    element.scrollTop = scrolledTo;
  }
</script>

{#snippet bar(leading: 'back' | 'close')}
  <div class="flex h-12 shrink-0 items-center gap-1 px-1.5 @card-roomy/card:px-3">
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
  <div
    data-testid="card-scroll"
    class="min-h-0 flex-1 overflow-y-auto overscroll-contain"
    onscroll={(event) => {
      scrolledTo = event.currentTarget.scrollTop;
    }}
    {@attach keepScroll}
  >
    <CardBody {card} {editing} {sheet} />
  </div>
  <form
    class="flex shrink-0 items-center gap-2 border-t bg-background px-3 pt-2 pb-[calc(0.5rem+var(--safe-bottom))] @card-roomy/card:px-6"
    onsubmit={(event) => {
      event.preventDefault();
    }}
  >
    <Avatar person={ME} decorative class="size-7" />
    <Input
      bind:value={draft}
      aria-label={T.writeComment}
      placeholder={T.writeComment}
      enterkeyhint="send"
      data-testid="comment-field"
      class="flex-1 pointer-coarse:h-touch pointer-coarse:text-base"
    />
    <IconButton type="submit" label={T.send}><SendIcon /></IconButton>
  </form>
{/snippet}

{#if form === 'dialog'}
  <Dialog.Root bind:open>
    <Dialog.Content
      bind:ref={content}
      showCloseButton={false}
      data-testid={DIALOG_NAME[stage]}
      class={['@container/card flex flex-col gap-0 overflow-hidden p-0', DIALOG[stage]]}
      onOpenAutoFocus={focusFrame}
    >
      <Dialog.Title class="sr-only">{card.title}</Dialog.Title>
      {@render bar(stage === 'compact' ? 'back' : 'close')}
      {@render body()}
    </Dialog.Content>
  </Dialog.Root>
{:else if form === 'sheet'}
  <Sheet.Root bind:open>
    <Sheet.Content
      bind:ref={content}
      side="bottom"
      showCloseButton={false}
      data-testid="card-sheet"
      class="@container/card gap-0 pr-(--safe-right) pl-(--safe-left) outline-none data-[side=bottom]:h-[calc(100dvh-var(--safe-top)-2.5rem)] data-[side=bottom]:rounded-t-2xl"
      onOpenAutoFocus={focusFrame}
    >
      <div
        aria-hidden="true"
        class="mx-auto mt-2 h-1 w-9 shrink-0 rounded-full bg-input-border"
      ></div>
      <Sheet.Title class="sr-only">{card.title}</Sheet.Title>
      {@render bar('close')}
      {@render body()}
    </Sheet.Content>
  </Sheet.Root>
{:else if open}
  <!-- In the row of the shell, after the pane of the board: half of the page, within limits. -->
  <aside
    data-testid="card-pane"
    aria-label={card.title}
    class="@container/card flex w-[clamp(22.5rem,50%,40rem)] shrink-0 flex-col border-l bg-background"
  >
    {@render bar('close')}
    {@render body()}
  </aside>
{/if}
