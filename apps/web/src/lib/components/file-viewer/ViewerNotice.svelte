<!--
  What the viewer (FileViewer.svelte) shows in the document's place when it
  cannot show the file: why, and the file offered as a download. Nothing for
  a status that is no such case.
-->
<script lang="ts">
  import DownloadIcon from '@lucide/svelte/icons/download';

  import { LIMITS, type Status } from '$lib/file-viewer/session.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { humanFileSize } from '$lib/logic/format.ts';

  interface Props {
    status: Status;
    onDownload: () => void;
  }

  let { status, onDownload }: Props = $props();

  /** What the viewer says when it cannot show the file. */
  const FAILURE: Record<Extract<Status, { name: 'failed' }>['reason'], () => string> = {
    unread: m.viewer_unread,
    unreadable: m.viewer_unreadable,
    locked: m.viewer_locked,
    'too-large': m.viewer_unpacks_too_large,
    slow: m.viewer_slow,
    unstarted: m.viewer_unstarted,
    engine: m.viewer_engine,
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
</script>

{#if notice}
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
      onclick={onDownload}
    >
      <DownloadIcon class="size-4" aria-hidden="true" />
      {m.viewer_download()}
    </button>
  </div>
{/if}
