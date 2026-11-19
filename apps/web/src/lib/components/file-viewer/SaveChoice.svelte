<!--
  What the editor (FileEditor.svelte) asks before a save that is not a plain
  new version, over the editor:

  - rhwp says the saved file loses something of the document: what, where
    and why, as its report names them, and the choice between a new version
    all the same, a file of its own beside this one, and not saving;
  - the file got a newer version than the one edited: the choice between
    keeping the edit as a file of its own and not saving.

  Not saving leaves the document as it is in the editor, its changes unsaved.
-->
<script lang="ts">
  import type { EditingSession } from '$lib/file-viewer/editing.svelte.ts';
  import type { Loss } from '$lib/file-viewer/editor-protocol.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import * as AlertDialog from '$lib/ui/alert-dialog/index.js';

  interface Props {
    session: EditingSession;
  }

  let { session }: Props = $props();

  const saving = $derived(session.saving);
  /** The report shown: kept while the dialog closes, so that it does not empty as it goes. */
  const lossy = $derived(saving.name === 'lossy' ? saving : undefined);

  const WHAT: Record<Loss['code'], () => string> = {
    binaryContentEmptied: m.file_editor_loss_binary,
    controlOmitted: m.file_editor_loss_control,
    metadataReduced: m.file_editor_loss_metadata,
  };
  const WHY: Record<Loss['reason'], () => string> = {
    resourceReadFailedOrLimitExceeded: m.file_editor_loss_reason_read,
    storedCompressionMismatch: m.file_editor_loss_reason_compression,
    rawPassthroughUnavailable: m.file_editor_loss_reason_passthrough,
    unsupportedByOutputFormat: m.file_editor_loss_reason_format,
  };
</script>

<AlertDialog.Root
  open={lossy !== undefined}
  onOpenChange={(open) => {
    if (!open) session.dismiss();
  }}
>
  <AlertDialog.Content data-testid="file-editor-losses">
    <AlertDialog.Header>
      <AlertDialog.Title>{m.file_editor_loss_title()}</AlertDialog.Title>
      <AlertDialog.Description>
        {m.file_editor_loss_description({ count: lossy?.count ?? 0 })}
      </AlertDialog.Description>
    </AlertDialog.Header>
    {#if lossy}
      <ul class="max-h-64 overflow-y-auto rounded-md border text-sm" tabindex="-1">
        {#each lossy.losses as loss, at (at)}
          <li class="border-b px-3 py-2 last:border-b-0">
            <p>{WHAT[loss.code]()}: {WHY[loss.reason]()}</p>
            <p class="font-mono text-xs break-all text-muted-foreground">{loss.path}</p>
          </li>
        {/each}
        {#if lossy.count > lossy.losses.length}
          <li class="px-3 py-2 text-muted-foreground">
            {m.file_editor_loss_more({ count: lossy.count - lossy.losses.length })}
          </li>
        {/if}
      </ul>
    {/if}
    <AlertDialog.Footer>
      <AlertDialog.Cancel class="roomy:min-h-touch">{m.shared_cancel()}</AlertDialog.Cancel>
      <AlertDialog.Action
        variant="outline"
        class="roomy:min-h-touch"
        data-testid="file-editor-losses-apart"
        onclick={() => {
          session.saveApart();
        }}
      >
        {m.file_version_save_apart()}
      </AlertDialog.Action>
      <AlertDialog.Action
        variant="destructive"
        class="roomy:min-h-touch"
        data-testid="file-editor-losses-version"
        onclick={() => {
          session.saveWithLosses();
        }}
      >
        {m.file_editor_save_as_version()}
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root
  open={saving.name === 'stale'}
  onOpenChange={(open) => {
    if (!open) session.dismiss();
  }}
>
  <AlertDialog.Content data-testid="file-editor-stale">
    <AlertDialog.Header>
      <AlertDialog.Title>{m.file_editor_stale_title()}</AlertDialog.Title>
      <AlertDialog.Description>{m.file_editor_stale_description()}</AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel class="roomy:min-h-touch">{m.shared_cancel()}</AlertDialog.Cancel>
      <AlertDialog.Action
        class="roomy:min-h-touch"
        data-testid="file-editor-stale-apart"
        onclick={() => {
          session.saveApart();
        }}
      >
        {m.file_version_save_apart()}
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
