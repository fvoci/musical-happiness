<!--
  The versions kept of a card's body, in a dialog the card's menu opens for
  the board's members: the list (when each was kept, and who wrote the body
  up to it), and the body of the one chosen, only to read. Who edits the
  board can go back to it, after being asked: the card's body becomes that
  version for everyone, and the dialog closes on the card. The version the
  card has now offers nothing to go back to.

  Wide, the list stays beside the body; narrow, the body takes the list's
  place, and a back arrow (and the back button) returns to it. The focus
  follows: to the arrow when the list gives way, and back to the version
  that was read.
-->
<script lang="ts">
  import { tick } from 'svelte';
  import { MediaQuery } from 'svelte/reactivity';

  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
  import XIcon from '@lucide/svelte/icons/x';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { displayDateTime, relativeTime } from '$lib/logic/format.ts';
  import type { BoardStore, BodyVersion } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import { bodyReferences } from './body-references.ts';
  import type { VersionsReading } from './body-versions.svelte.ts';

  interface Props {
    open: boolean;
    /** Made when the dialog opens: a new reading each time. */
    reading: VersionsReading | undefined;
    board: BoardStore;
  }

  let { open = $bindable(), reading, board }: Props = $props();

  const references = $derived(bodyReferences(board));

  /** How many faces a version shows of those who wrote it. */
  const FACES = 3;

  let asking = $state(false);

  const workspace = $derived(board.workspace);
  const context = $derived({
    locale: workspace.locale,
    timeZone: workspace.regional.timeZone,
    hourCycle: workspace.regional.hourCycle,
  });
  const names = $derived(new Intl.ListFormat(workspace.locale, { type: 'conjunction' }));
  const chosen = $derived(reading?.chosen);
  /** Whether the card has the chosen body now, as its reading says, or the list before it. */
  const current = $derived(reading?.content?.isCurrent ?? chosen?.isCurrent ?? false);

  const resolveUser = (userId: string): string =>
    workspace.userName(userId) ?? m['centerPanel.unknown-user']();
  const whoWrote = (version: BodyVersion): string => names.format(version.editors.map(resolveUser));
  const whenKept = (version: BodyVersion): string => displayDateTime(version.keptAt, context);

  /** Narrow: the body of a version takes the place of the list (Tailwind's `sm`). */
  const narrow = new MediaQuery('(max-width: 39.99rem)');

  // The back button closes it, as the other dialogs: the card under it stays.
  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );
  // Narrow, with a version open, it goes back to the list first.
  closeOnBack(
    () => open && narrow.current && chosen !== undefined,
    () => reading?.leave(),
  );

  /** The dialog a row or the back arrow is in: where the focus is sent on within. */
  const dialogOf = (event: MouseEvent): Element | null =>
    (event.currentTarget as HTMLElement).closest('[data-testid="body-versions"]');

  async function read(event: MouseEvent, version: BodyVersion): Promise<void> {
    const row = event.currentTarget as HTMLElement;
    const dialog = dialogOf(event);
    void reading?.choose(version);
    await tick();
    // The list gave way to the body, and the row that had the focus with it.
    if (row.offsetParent === null) {
      dialog?.querySelector<HTMLElement>('[data-testid="body-versions-back"]')?.focus();
    }
  }

  async function backToList(event: MouseEvent): Promise<void> {
    const dialog = dialogOf(event);
    const left = chosen?.version;
    reading?.leave();
    await tick();
    dialog?.querySelector<HTMLElement>(`[data-version="${String(left)}"]`)?.focus();
  }

  async function restore(): Promise<void> {
    if (await reading?.restore()) open = false;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    class="flex h-[min(90dvh,44rem)] flex-col gap-0 p-0 sm:max-w-3xl"
    showCloseButton={false}
    data-testid="body-versions"
  >
    <Dialog.Header class="flex-row items-center gap-2 border-b p-4">
      {#if chosen}
        <IconButton
          label={m.card_versions_list()}
          class="sm:hidden"
          data-testid="body-versions-back"
          onclick={backToList}
        >
          <ArrowLeftIcon aria-hidden="true" />
        </IconButton>
      {/if}
      <div class="min-w-0 flex-1">
        <Dialog.Title>{m.card_versions_title()}</Dialog.Title>
        <Dialog.Description class={[chosen && 'max-sm:sr-only']}>
          {m.card_versions_description()}
        </Dialog.Description>
      </div>
      <Dialog.Close>
        {#snippet child({ props }: { props: Record<string, unknown> })}
          <IconButton {...props} label={m.ui_close()}>
            <XIcon aria-hidden="true" />
          </IconButton>
        {/snippet}
      </Dialog.Close>
    </Dialog.Header>

    {#if reading}
      <div class="flex min-h-0 flex-1">
        <div
          class={[
            'min-h-0 w-full overflow-y-auto overscroll-contain sm:w-72 sm:shrink-0 sm:border-e',
            chosen && 'max-sm:hidden',
          ]}
        >
          {#if reading.status === 'loading'}
            <p class="p-4 text-muted-foreground" role="status">{m.app_loading()}</p>
          {:else if reading.status === 'failed'}
            <div class="grid justify-items-start gap-2 p-4">
              <p role="alert">{m.card_versions_failed()}</p>
              <Button variant="outline" class="roomy:min-h-touch" onclick={() => reading.read()}>
                {m.app_retry()}
              </Button>
            </div>
          {:else if reading.versions.length === 0}
            <p class="p-4 text-muted-foreground" data-testid="body-versions-empty">
              {m.card_versions_empty()}
            </p>
          {:else}
            <ol class="grid p-1.5" data-testid="body-versions-list">
              {#each reading.versions as version (version.version)}
                {@const when = whenKept(version)}
                <li>
                  <button
                    type="button"
                    class="grid w-full gap-1 rounded-md px-2.5 py-2 text-start outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring aria-[current=true]:bg-muted roomy:min-h-touch"
                    aria-current={chosen?.version === version.version}
                    disabled={reading.restoring}
                    data-version={version.version}
                    onclick={(event) => read(event, version)}
                  >
                    <span class="flex items-center gap-2">
                      <time class="font-medium" datetime={new Date(version.keptAt).toISOString()}>
                        {relativeTime(version.keptAt, workspace.locale)}
                      </time>
                      {#if version.isCurrent}
                        <span
                          class="rounded-sm bg-primary/10 px-1.5 py-0.5 text-xs font-medium text-primary"
                          data-testid="body-version-current"
                        >
                          {m.card_version_current()}
                        </span>
                      {/if}
                    </span>
                    <span class="text-xs text-muted-foreground">{when}</span>
                    {#if version.editors.length > 0}
                      <span class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                        <span class="flex shrink-0 -space-x-1">
                          {#each version.editors.slice(0, FACES) as userId (userId)}
                            <Avatar
                              name={resolveUser(userId)}
                              colorKey={userId}
                              locale={workspace.locale}
                              decorative
                              class="size-5 ring-2 ring-popover"
                            />
                          {/each}
                        </span>
                        <span class="truncate" data-testid="body-version-editors">
                          {whoWrote(version)}
                        </span>
                      </span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ol>
          {/if}
        </div>

        <section
          class={['min-h-0 min-w-0 flex-1 flex-col', chosen ? 'flex' : 'hidden sm:flex']}
          aria-label={chosen ? m.card_version_read({ when: whenKept(chosen) }) : undefined}
          data-testid="body-version"
        >
          {#if !chosen}
            <p class="m-auto max-w-xs p-6 text-center text-muted-foreground">
              {reading.versions.length > 0 ? m.card_versions_pick() : ''}
            </p>
          {:else}
            <p class="border-b px-4 py-2.5 text-xs text-muted-foreground">
              <span class="font-medium text-foreground">{whenKept(chosen)}</span>
              {#if chosen.editors.length > 0}
                · {m.card_version_by({ who: whoWrote(chosen) })}
              {/if}
            </p>
            <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-3">
              {#if reading.contentStatus === 'loading'}
                <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
              {:else if reading.contentStatus === 'gone'}
                <p role="alert">{m.card_version_gone()}</p>
              {:else if reading.contentStatus === 'failed'}
                <p role="alert">{m.card_version_read_failed()}</p>
              {:else}
                <BodyView
                  doc={reading.content?.body ?? null}
                  {resolveUser}
                  {references}
                  class="text-base"
                >
                  {#snippet empty()}
                    <p class="text-muted-foreground">{m.card_version_no_body()}</p>
                  {/snippet}
                  {#snippet invalid()}
                    <p class="text-muted-foreground" role="alert">{m.card_body_invalid()}</p>
                  {/snippet}
                </BodyView>
              {/if}
            </div>
            {#if board.canEdit && reading.contentStatus === 'ready'}
              <div class="flex flex-wrap items-center justify-end gap-x-3 gap-y-2 border-t p-3">
                {#if reading.refusal}
                  <p class="min-w-0 flex-1 text-sm" role="alert" data-refusal={reading.refusal}>
                    {reading.refusal === 'gone'
                      ? m.card_version_gone()
                      : reading.refusal === 'unrestorable'
                        ? m.card_version_unrestorable()
                        : m.card_version_restore_failed()}
                  </p>
                {/if}
                {#if current}
                  <p class="text-sm text-muted-foreground" data-testid="body-version-is-current">
                    {m.card_version_is_current()}
                  </p>
                {:else}
                  <Button
                    class="roomy:min-h-touch"
                    disabled={reading.restoring}
                    data-testid="body-version-restore"
                    onclick={() => {
                      asking = true;
                    }}
                  >
                    <RotateCcwIcon aria-hidden="true" />
                    {m.card_version_restore()}
                  </Button>
                {/if}
              </div>
            {/if}
          {/if}
        </section>
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<ConfirmDialog
  bind:open={asking}
  title={m.card_version_restore_title()}
  description={chosen ? m.card_version_restore_description({ when: whenKept(chosen) }) : undefined}
  confirmLabel={m.card_version_restore_confirm()}
  onConfirm={() => void restore()}
/>
