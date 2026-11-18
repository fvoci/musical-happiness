<!--
  A file shown in the app instead of downloaded: a PDF, a Word document or a
  Hangul (HWP, HWPX) document, drawn in a frame that has no origin and no network
  (lib/file-viewer/session.svelte.ts, src/viewer-frame/).

  A dialog over the page on a wide screen, the whole screen on a narrow one.
  Pages and zoom are at the bottom, within reach of a thumb. The keys, while
  the focus is in the viewer or in the document: the left and right arrows
  for the previous and next page, + and − to zoom, 0 for 100%, Escape to
  close. A file that cannot be shown here says why and offers itself as a
  download.

  Where the file has versions to show (`versions`: not for who reads by a
  public link), a button in the header opens their list (FileVersions.svelte)
  beside the document, or over it on a narrow screen. A version picked there
  is shown in the file's place, in a frame of its own; the header says when
  what is shown is not the newest.

  It is mounted for one file and closed by unmounting it, which takes the
  frame out of the page and stops whatever the frame was doing.
-->
<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import HistoryIcon from '@lucide/svelte/icons/history';
  import XIcon from '@lucide/svelte/icons/x';
  import ZoomInIcon from '@lucide/svelte/icons/zoom-in';
  import ZoomOutIcon from '@lucide/svelte/icons/zoom-out';
  import { tick, untrack } from 'svelte';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { display } from '$lib/display.svelte.ts';
  import {
    frameAddress,
    LIMITS,
    ViewerSession,
    type Status,
    type ViewedFile,
  } from '$lib/file-viewer/session.svelte.ts';
  import {
    VersionsList,
    type FileVersion,
    type VersionsSource,
  } from '$lib/file-viewer/versions.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import { humanFileSize } from '$lib/logic/format.ts';
  import { saveBlob } from '$lib/save.ts';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import FileVersions from './FileVersions.svelte';

  interface Props {
    /** The file; the viewer is made for one and is not handed another. */
    file: ViewedFile;
    /** The file's versions, where the reader may see them. */
    versions?: VersionsSource | undefined;
    onClose: () => void;
  }

  let { file, versions, onClose }: Props = $props();

  const uid = $props.id();

  const sessionOf = (viewed: ViewedFile): ViewerSession =>
    new ViewerSession(viewed, {
      theme: () => display.resolved.theme,
      lang: getLocale,
      onKey: (key) => {
        command(key);
      },
    });

  /** What is on screen: the file as it was opened, then whichever version was picked. */
  let session = $state.raw(untrack(() => sessionOf(file)));
  /** The version picked from the list, if one was. */
  let picked = $state.raw<FileVersion>();
  /** The list of versions, made when it is first opened. */
  let list = $state.raw<VersionsList>();
  let listed = $state(false);
  let panel = $state<HTMLElement>();

  /** The number of the version on screen, once the list has said which the file was opened as. */
  const shownVersion = $derived(picked?.version ?? list?.opened);
  /** What is on screen is not the file's newest version. */
  const older = $derived(
    shownVersion !== undefined &&
      list?.current !== undefined &&
      shownVersion !== list.current.version,
  );
  const extension = $derived(/\.[^./\\]+$/.exec(file.name)?.[0] ?? '');

  const address = frameAddress(document);
  const compact = $derived(page.stage === 'compact');
  const status = $derived(session.status);
  const framed = $derived(status.name === 'opening' || status.name === 'shown');

  closeOnBack(
    () => true,
    () => {
      onClose();
    },
  );
  // Narrow, the list is over the document: back shows the document again.
  closeOnBack(
    () => compact && listed,
    () => {
      closeVersions();
    },
  );

  $effect(() => {
    const theme = display.resolved.theme;
    // Only the theme sends it again; the session's status is read, not followed.
    untrack(() => {
      session.setTheme(theme);
    });
  });

  // A session ends when another takes its place, and with the viewer.
  $effect(() => {
    const current = session;
    return () => {
      current.close();
    };
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
    void session.file.read().then((blob) => {
      if (blob) saveBlob(blob, file.name);
    });
  }

  /** Shows a version in the file's place. */
  function view(version: FileVersion): void {
    const source = versions;
    if (!source) return;
    picked = version;
    session = sessionOf({
      name: file.name,
      size: version.size,
      read: (signal) => source.bytes(version.version, signal),
    });
    // Narrow, the document is under the list.
    if (compact) closeVersions();
  }

  function downloadVersion(version: FileVersion): void {
    void versions?.bytes(version.version).then((blob) => {
      if (blob) saveBlob(blob, file.name);
    });
  }

  async function openVersions(): Promise<void> {
    if (!versions) return;
    list ??= new VersionsList(versions);
    listed = true;
    await tick();
    panel?.querySelector<HTMLElement>('h2')?.focus();
  }

  function closeVersions(): void {
    listed = false;
    // The list is gone with its focus: back to the button it was opened from.
    document.getElementById(`${uid}-versions`)?.focus();
  }

  function command(key: string): boolean {
    switch (key) {
      case 'ArrowLeft':
        session.goTo(session.page - 1);
        return true;
      case 'ArrowRight':
        session.goTo(session.page + 1);
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
  >
    <header class="flex min-h-12 items-center gap-1 border-b py-1 pr-1 pl-4">
      <Dialog.Title class="min-w-0 flex-1 truncate text-base font-semibold"
        >{file.name}</Dialog.Title
      >
      <Dialog.Description class="sr-only">{m.viewer_description()}</Dialog.Description>
      {#if older}
        <p
          class="shrink-0 rounded-sm border px-1.5 py-0.5 text-xs font-medium"
          data-testid="file-viewer-older"
        >
          {m.viewer_older_version()}
        </p>
      {/if}
      {#if versions}
        <IconButton
          id="{uid}-versions"
          label={m.viewer_versions()}
          aria-expanded={listed}
          data-testid="file-viewer-versions"
          onclick={() => {
            if (listed) closeVersions();
            else void openVersions();
          }}
        >
          <HistoryIcon aria-hidden="true" />
        </IconButton>
      {/if}
      <IconButton label={m.viewer_download()} data-testid="file-viewer-download" onclick={download}>
        <DownloadIcon aria-hidden="true" />
      </IconButton>
      <IconButton label={m.ui_close()} data-testid="file-viewer-close" onclick={onClose}>
        <XIcon aria-hidden="true" />
      </IconButton>
    </header>

    <div class="relative flex min-h-0 flex-1">
      <div class="relative min-h-0 min-w-0 flex-1 bg-muted">
        {#if framed}
          <!-- A frame is one session's: another version is drawn in a new one. -->
          {#key session}
            <iframe
              {@attach session.attach}
              src={address}
              sandbox="allow-scripts"
              referrerpolicy="no-referrer"
              title={m.viewer_frame_title({ name: file.name })}
              class={['size-full border-0', status.name !== 'shown' && 'invisible']}
              data-testid="file-viewer-frame"
            ></iframe>
          {/key}
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
      {#if listed && list && versions}
        <div
          bind:this={panel}
          class={compact ? 'absolute inset-0 z-10 flex' : 'flex w-80 shrink-0 border-s'}
        >
          <FileVersions
            {list}
            source={versions}
            shown={shownVersion}
            accept={extension}
            onView={view}
            onDownload={downloadVersion}
            onWritten={view}
            onClose={closeVersions}
          />
        </div>
      {/if}
    </div>

    <!-- In the page from the start, so that the first page is announced as well as the next. -->
    <p class="sr-only" aria-live="polite" data-testid="file-viewer-announce">
      {status.name === 'shown' ? m.viewer_page({ page: session.page, pages: session.pages }) : ''}
    </p>

    {#if status.name === 'shown'}
      <footer class="flex min-h-12 items-center justify-center gap-1 border-t p-1">
        <IconButton
          label={m.viewer_previous_page()}
          disabled={session.page <= 1}
          onclick={() => {
            session.goTo(session.page - 1);
          }}
        >
          <ChevronLeftIcon aria-hidden="true" />
        </IconButton>
        <p class="min-w-16 text-center text-sm tabular-nums" data-testid="file-viewer-page">
          <span aria-hidden="true">{session.page} / {session.pages}</span>
          <span class="sr-only">{m.viewer_page({ page: session.page, pages: session.pages })}</span>
        </p>
        <IconButton
          label={m.viewer_next_page()}
          disabled={session.page >= session.pages}
          onclick={() => {
            session.goTo(session.page + 1);
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
