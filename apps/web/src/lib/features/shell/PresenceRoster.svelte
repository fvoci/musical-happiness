<!--
  Who is on the board now and how many members it has, at the head of the board.

  The faces of the others here now (five, two in a narrow pane, then "+k" for the
  rest and for those the reader may not see; in a narrow pane also for the anonymous
  readers of its public link) open the list of who is here and
  which cards they have open. Beside them the number of the board's members opens
  the members of the sharing dialog (`onMembers`); it is there also when nobody
  else is, in a narrow pane as an icon and the number alone ("Members 4" for a
  screen reader still), which leaves the title room. A screen reader hears both
  numbers together on the group.
-->
<script lang="ts">
  import UsersIcon from '@lucide/svelte/icons/users';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { boardPresence, type BoardStore } from '$lib/state/index.ts';

  let { board, onMembers }: { board: BoardStore; onMembers: () => void } = $props();
  const presence = $derived(boardPresence(board));
  const workspace = $derived(board.workspace);
  const presentation = new PickerPresentation();
  const label = $derived(m.presence_viewers({ count: presence.count }));
  /** Everybody the members' list shows: each member is somebody the reader may see. */
  const members = $derived(board.orderedMembers.length);
  /** How many faces are shown before the rest become a number; fewer in a narrow pane. */
  const FACES = 5;
  const NARROW_FACES = 2;
  const faces = $derived(presence.others.slice(0, FACES));
  /** The people not shown as faces: past those shown, and those the reader may not see. */
  const moreThan = (shown: number): number =>
    Math.max(presence.others.length - shown, 0) + presence.snapshot.hiddenCount;
  const more = $derived(moreThan(FACES));
  const narrowMore = $derived(moreThan(NARROW_FACES) + presence.snapshot.anonymousCount);
</script>

<div
  role="group"
  aria-label={m.presence_and_members({ count: presence.count, members })}
  class="flex shrink-0 items-center gap-1"
  data-testid="board-people"
>
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
          {#each faces as person, index (person.userId)}
            <Avatar
              name={workspace.userName(person.userId) ?? m['centerPanel.unknown-user']()}
              colorKey={person.userId}
              locale={workspace.locale}
              decorative
              class="size-5 ring-2 ring-background @columns/pane:size-6 {index >= NARROW_FACES
                ? '@max-columns/pane:hidden'
                : ''}"
            />
          {/each}
        </span>
        {#if narrowMore > 0}<span class="@columns/pane:hidden">+{narrowMore}</span>{/if}
        {#if more > 0}<span class="hidden @columns/pane:inline">+{more}</span>{/if}
        {#if presence.snapshot.anonymousCount > 0}
          <span class="hidden @columns/pane:inline">
            {m.presence_anonymous({ count: presence.snapshot.anonymousCount })}
          </span>
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
  <button
    type="button"
    data-testid="board-members-count"
    aria-haspopup="dialog"
    aria-label={m.presence_members_short({ count: members })}
    title={m.presence_members_short({ count: members })}
    class="flex min-h-8 shrink-0 items-center gap-0.5 rounded-md px-1 text-xs whitespace-nowrap text-muted-foreground hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring roomy:min-h-touch"
    onclick={onMembers}
  >
    <UsersIcon aria-hidden="true" class="size-3.5 @columns/pane:hidden" />
    <span class="@columns/pane:hidden">{members}</span>
    <span class="@max-columns/pane:hidden">{m.presence_members_short({ count: members })}</span>
  </button>
</div>
