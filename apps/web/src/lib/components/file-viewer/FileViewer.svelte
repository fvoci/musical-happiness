<!--
  A file shown in the app instead of downloaded: a PDF, a Word document, a
  workbook, a PowerPoint deck or a Hangul (HWP, HWPX) document, drawn in a
  frame that has no origin and no network (lib/file-viewer/session.svelte.ts,
  src/viewer-frame/). A workbook's pages are its sheets, named in tabs under
  it and in what is announced; a deck's are its slides. A deck with slides
  hidden from a show opens on the others, under a line that counts them and
  a button that shows the hidden ones in place (marked in the frame and in
  what is announced); a slide keeps its number, which jumps where slides
  are left out.

  A dialog over the page on a wide screen, the whole screen on a narrow one.
  Pages and zoom are at the bottom, within reach of a thumb. The keys, while
  the focus is in the viewer or in the document: the left and right arrows
  for the previous and next page, + and − to zoom, 0 for 100%, Escape to
  close. A file that cannot be shown here says why and offers itself as a
  download.

  It is mounted for one file and closed by unmounting it, which takes the
  frame out of the page and stops whatever the frame was doing.
-->
<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import XIcon from '@lucide/svelte/icons/x';
  import ZoomInIcon from '@lucide/svelte/icons/zoom-in';
  import ZoomOutIcon from '@lucide/svelte/icons/zoom-out';
  import { untrack } from 'svelte';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';

  import SheetTabs from './SheetTabs.svelte';
  import { display } from '$lib/display.svelte.ts';
  import {
    frameAddress,
    LIMITS,
    ViewerSession,
    type Status,
    type ViewedFile,
  } from '$lib/file-viewer/session.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import { humanFileSize } from '$lib/logic/format.ts';
  import { saveBlob } from '$lib/save.ts';
  import * as Dialog from '$lib/ui/dialog/index.js';

  interface Props {
    /** The file; the viewer is made for one and is not handed another. */
    file: ViewedFile;
    onClose: () => void;
    /**
     * Gives the focus back as the viewer goes, once its hold on the focus is let go of; without
     * it, the focus goes back to what had it when the viewer opened.
     */
    returnFocus?: () => void;
  }

  let { file, onClose, returnFocus }: Props = $props();

  const session = untrack(
    () =>
      new ViewerSession(file, {
        theme: () => display.resolved.theme,
        lang: getLocale,
        onKey: (key) => {
          command(key);
        },
        onDownload: () => {
          download();
        },
      }),
  );
  const address = frameAddress(document);
  const compact = $derived(page.stage === 'compact');
  const status = $derived(session.status);
  const framed = $derived(status.name === 'opening' || status.name === 'shown');
  /** The sheet shown, when the file is a workbook. */
  const sheet = $derived(session.sheets[session.page - 1]);
  /** A deck's pages are its slides, and are called so. */
  const slides = $derived(status.name === 'shown' && status.format === 'pptx');
  /** A deck's slides, when some are hidden from a show. */
  const order = $derived(slides ? session.slides : undefined);
  const onHidden = $derived(order?.hidden.has(session.page) === true);
  const where = $derived(
    sheet
      ? m.viewer_sheet({ page: session.page, pages: session.pages, name: sheet.name })
      : (onHidden ? m.viewer_slide_hidden : slides ? m.viewer_slide : m.viewer_page)({
          page: session.page,
          pages: session.pages,
        }),
  );
  /** How many slides are laid out, and how many of them are hidden. */
  const slideCount = $derived.by((): string => {
    if (!order) return '';
    const count = order.count(session.showHidden);
    if (order.allHidden) return m.viewer_slides_all_hidden({ count });
    return session.showHidden
      ? m.viewer_slides_count_with_hidden({ count, hidden: order.hidden.size })
      : m.viewer_slides_count({ count });
  });
  /** What is said of the page turned to: of a sheet with drawings, that they are not shown. */
  const announced = $derived(sheet?.drawings ? `${where} ${m.viewer_sheet_drawings()}` : where);
  const previous = $derived(
    sheet
      ? m.viewer_previous_sheet()
      : slides
        ? m.viewer_previous_slide()
        : m.viewer_previous_page(),
  );
  const next = $derived(
    sheet ? m.viewer_next_sheet() : slides ? m.viewer_next_slide() : m.viewer_next_page(),
  );

  closeOnBack(
    () => true,
    () => {
      onClose();
    },
  );

  $effect(() => {
    const theme = display.resolved.theme;
    // Only the theme sends it again; the session's status is read, not followed.
    untrack(() => {
      session.setTheme(theme);
    });
  });

  $effect(() => () => {
    session.close();
  });

  /** What the viewer says when it cannot show the file. */
  const FAILURE: Record<Extract<Status, { name: 'failed' }>['reason'], () => string> = {
    unread: m.viewer_unread,
    unreadable: m.viewer_unreadable,
    locked: m.viewer_locked,
    'too-large': m.viewer_unpacks_too_large,
    slow: m.viewer_slow,
    renderer: m.viewer_failed,
  };

  /** What the viewer says instead of the document, if anything. */
  const notice = $derived.by((): string | undefined => {
    if (status.name === 'too-large')
      return status.what === 'file'
        ? m.viewer_too_large({ size: humanFileSize(LIMITS.bytes) })
        : m.viewer_unpacks_too_large();
    if (status.name === 'unknown') return m.viewer_unknown();
    if (status.name === 'failed') return FAILURE[status.reason]();
    return undefined;
  });

  function download(): void {
    if (session.blob) {
      saveBlob(session.blob, file.name);
      return;
    }
    void file.read().then((blob) => {
      if (blob) saveBlob(blob, file.name);
    });
  }

  function command(key: string): boolean {
    switch (key) {
      case 'ArrowLeft':
        session.turn(-1);
        return true;
      case 'ArrowRight':
        session.turn(1);
        return true;
      case '+':
      case '=':
        session.zoomBy(1);
        return true;
      case '-':
        session.zoomBy(-1);
        return true;
      case '0':
        session.zoomBy(0);
        return true;
      case 'Escape':
        onClose();
        return true;
      default:
        return false;
    }
  }

  function onkeydown(event: KeyboardEvent): void {
    // Escape is the dialog's own; keys typed into a control are the control's.
    if (event.key === 'Escape' || event.ctrlKey || event.metaKey || event.altKey) return;
    // With Shift, the arrows across are the frame's, which scrolls across with them where it has
    // the focus: they turn no page from the viewer's buttons either.
    if (event.shiftKey && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) return;
    if (command(event.key)) event.preventDefault();
  }
</script>

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) onClose();
  }}
>
  <Dialog.Content
    showCloseButton={false}
    data-testid="file-viewer"
    data-status={status.name}
    data-frame={compact ? 'full' : 'dialog'}
    class={[
      'flex flex-col gap-0 overflow-hidden bg-background p-0',
      compact
        ? 'top-0 left-0 h-dvh w-full max-w-none translate-x-0 translate-y-0 rounded-none pt-(--safe-top) pr-(--safe-right) pb-(--safe-bottom) pl-(--safe-left) ring-0 sm:max-w-none'
        : 'h-[min(calc(100dvh-4rem),64rem)] max-w-[min(64rem,calc(100%-2rem))] sm:max-w-[min(64rem,calc(100%-2rem))]',
    ]}
    {onkeydown}
    onCloseAutoFocus={(event: Event) => {
      if (!returnFocus) return;
      event.preventDefault();
      returnFocus();
    }}
  >
    <header class="flex min-h-12 items-center gap-1 border-b py-1 pr-1 pl-4">
      <Dialog.Title class="min-w-0 flex-1 truncate text-base font-semibold"
        >{file.name}</Dialog.Title
      >
      <Dialog.Description class="sr-only">{m.viewer_description()}</Dialog.Description>
      <IconButton label={m.viewer_download()} data-testid="file-viewer-download" onclick={download}>
        <DownloadIcon aria-hidden="true" />
      </IconButton>
      <IconButton label={m.ui_close()} data-testid="file-viewer-close" onclick={onClose}>
        <XIcon aria-hidden="true" />
      </IconButton>
    </header>
    {#if status.name === 'shown' && order}
      <div
        class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b px-4 py-1 text-sm"
        data-testid="file-viewer-slides"
      >
        <p class="text-muted-foreground" role="status">{slideCount}</p>
        {#if !order.allHidden}
          <button
            type="button"
            class="min-h-8 rounded-md px-2 font-medium hover:bg-accent roomy:min-h-touch"
            data-testid="file-viewer-hidden"
            onclick={() => {
              session.toggleHidden();
            }}
          >
            {session.showHidden
              ? m.viewer_hide_hidden()
              : m.viewer_show_hidden({ count: order.hidden.size })}
          </button>
        {/if}
      </div>
    {/if}

    <div class="relative min-h-0 flex-1 bg-muted">
      {#if framed}
        <iframe
          {@attach session.attach}
          src={address}
          sandbox="allow-scripts"
          referrerpolicy="no-referrer"
          title={m.viewer_frame_title({ name: file.name })}
          class={['size-full border-0', status.name !== 'shown' && 'invisible']}
          data-testid="file-viewer-frame"
        ></iframe>
      {/if}
      {#if status.name === 'reading' || status.name === 'opening'}
        <p
          class="absolute inset-0 flex items-center justify-center text-muted-foreground"
          role="status"
        >
          {m.viewer_opening()}
        </p>
      {:else if notice}
        <div
          class="absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
          role="status"
          data-testid="file-viewer-notice"
        >
          <p class="font-medium">{notice}</p>
          <p class="text-muted-foreground">{m.viewer_download_instead()}</p>
          <button
            type="button"
            class="inline-flex min-h-9 items-center gap-2 rounded-md border bg-background px-3 hover:bg-accent roomy:min-h-touch"
            onclick={download}
          >
            <DownloadIcon class="size-4" aria-hidden="true" />
            {m.viewer_download()}
          </button>
        </div>
      {/if}
    </div>

    <!-- In the page from the start, so that the first page is announced as well as the next. -->
    <p class="sr-only" aria-live="polite" data-testid="file-viewer-announce">
      {status.name === 'shown' ? announced : ''}
    </p>

    {#if status.name === 'shown' && session.sheets.length > 0}
      <SheetTabs
        sheets={session.sheets}
        current={session.page}
        onPick={(picked: number) => {
          session.goTo(picked);
        }}
      />
    {/if}
    {#if status.name === 'shown'}
      <footer class="flex min-h-12 items-center justify-center gap-1 border-t p-1">
        <IconButton
          label={previous}
          disabled={!session.canTurn(-1)}
          onclick={() => {
            session.turn(-1);
          }}
        >
          <ChevronLeftIcon aria-hidden="true" />
        </IconButton>
        <p class="min-w-16 text-center text-sm tabular-nums" data-testid="file-viewer-page">
          <span aria-hidden="true">{session.page} / {session.pages}</span>
          <span class="sr-only">{where}</span>
        </p>
        <IconButton
          label={next}
          disabled={!session.canTurn(1)}
          onclick={() => {
            session.turn(1);
          }}
        >
          <ChevronRightIcon aria-hidden="true" />
        </IconButton>
        <span class="mx-2 h-6 w-px bg-border" aria-hidden="true"></span>
        <IconButton
          label={m.viewer_zoom_out()}
          disabled={!session.canZoomOut}
          onclick={() => {
            session.zoomBy(-1);
          }}
        >
          <ZoomOutIcon aria-hidden="true" />
        </IconButton>
        <button
          type="button"
          class="min-h-8 min-w-14 rounded-md text-sm tabular-nums hover:bg-accent roomy:min-h-touch"
          aria-label={m.viewer_zoom_reset({ percent: Math.round(session.zoom * 100) })}
          data-testid="file-viewer-zoom"
          onclick={() => {
            session.zoomBy(0);
          }}
        >
          {Math.round(session.zoom * 100)}%
        </button>
        <IconButton
          label={m.viewer_zoom_in()}
          disabled={!session.canZoomIn}
          onclick={() => {
            session.zoomBy(1);
          }}
        >
          <ZoomInIcon aria-hidden="true" />
        </IconButton>
      </footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
