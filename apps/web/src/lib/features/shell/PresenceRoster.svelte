<script lang="ts">
  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { boardPresence, type BoardStore } from '$lib/state/index.ts';

  let { board }: { board: BoardStore } = $props();
  const presence = $derived(boardPresence(board));
  const workspace = $derived(board.workspace);
  const presentation = new PickerPresentation();
  const label = $derived(m.presence_viewers({ count: presence.count }));
  /** How many faces are shown before the rest become a number. */
  const FACES = 5;
  const faces = $derived(presence.others.slice(0, FACES));
  /** The people not shown as faces: past the fifth, and those the reader may not see. */
  const more = $derived(
    Math.max(presence.others.length - FACES, 0) + presence.snapshot.hiddenCount,
  );
</script>

<PickerSurface presentation={presentation.current} title={label}>
  {#snippet trigger({ props })}
    <button
      {...props}
      type="button"
      data-testid="board-presence"
      aria-label={label}
      class="flex min-h-8 shrink-0 items-center gap-1 rounded-md px-1 text-xs hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring roomy:min-h-touch"
    >
      <span class="flex -space-x-1">
        {#each faces as person (person.userId)}
          <Avatar
            name={workspace.userName(person.userId) ?? m['centerPanel.unknown-user']()}
            colorKey={person.userId}
            locale={workspace.locale}
            decorative
            class="size-5 ring-2 ring-background @columns/pane:size-6"
          />
        {/each}
      </span>
      {#if more > 0}<span>+{more}</span>{/if}
      {#if presence.snapshot.anonymousCount > 0}
        <span>{m.presence_anonymous({ count: presence.snapshot.anonymousCount })}</span>
      {/if}
      {#if presence.count === 0}<span aria-hidden="true">0</span>{/if}
    </button>
  {/snippet}
  <ul class="max-w-80 space-y-2 p-2" data-testid="presence-list">
    {#each presence.others as person (person.userId)}
      <li class="flex items-start gap-2" data-presence-user={person.userId}>
        <Avatar
          name={workspace.userName(person.userId) ?? m['centerPanel.unknown-user']()}
          colorKey={person.userId}
          locale={workspace.locale}
          decorative
        />
        <div class="min-w-0">
          <p class="break-words">
            {workspace.userName(person.userId) ?? m['centerPanel.unknown-user']()}
          </p>
          {#each person.cardIds as cardId (cardId)}
            <p class="truncate text-xs text-muted-foreground">
              {board.card(cardId)?.title || m['KanbanCard.untitled']()}
            </p>
          {/each}
        </div>
      </li>
    {/each}
    {#if presence.snapshot.anonymousCount > 0}
      <li>{m.presence_anonymous({ count: presence.snapshot.anonymousCount })}</li>
    {/if}
    {#if presence.snapshot.hiddenCount > 0}
      <li>{m.presence_hidden({ count: presence.snapshot.hiddenCount })}</li>
    {/if}
    {#if presence.count === 0}<li class="text-muted-foreground">{m.presence_alone()}</li>{/if}
  </ul>
</PickerSurface>
