<!--
  The comments of a card, oldest first: who wrote it, how long ago, and the
  text, drawn with the read-only view of a body. A comment somebody else
  writes while the card is open appears by itself (the card store follows
  the board's changes) and is announced.

  The reader can delete their own comments; "Undo" in the card's menu brings
  one back. The field for a new comment is not here: it is pinned to the
  bottom edge of the card (CommentComposer).
-->
<script lang="ts">
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import { onMount } from 'svelte';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { displayDateTime, relativeTime } from '$lib/logic/format.ts';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';

  interface Props {
    board: BoardStore;
    card: CardStore;
  }

  let { board, card }: Props = $props();

  const uid = $props.id();
  const workspace = $derived(board.workspace);
  const resolveUser = (userId: string): string | undefined => workspace.userName(userId);

  /** The current moment, for "3 minutes ago". Moves on once a minute. */
  let now = $state(Date.now());
  onMount(() => {
    const clock = setInterval(() => {
      now = Date.now();
    }, 60_000);
    return () => {
      clearInterval(clock);
    };
  });

  const context = $derived({
    locale: workspace.locale,
    timeZone: workspace.regional.timeZone,
    hourCycle: workspace.regional.hourCycle,
  });
</script>

<section
  aria-labelledby="{uid}-heading"
  class="flex flex-col gap-4 border-t pt-5"
  data-testid="card-comments"
>
  <h3 id="{uid}-heading" class="font-semibold">
    {m['CardBadges.title-comments']()}
    <span class="font-normal text-muted-foreground">{card.comments.length}</span>
  </h3>
  {#if card.comments.length === 0}
    <p class="text-muted-foreground">{m.card_comments_none()}</p>
  {/if}
  <ol class="flex flex-col gap-4 empty:hidden" aria-live="polite" aria-relevant="additions">
    {#each card.comments as comment (comment.id)}
      {@const author = workspace.userName(comment.createdBy) ?? m['centerPanel.unknown-user']()}
      <li class="flex gap-3" data-comment={comment.id}>
        <Avatar
          name={author}
          colorKey={comment.createdBy}
          locale={workspace.locale}
          decorative
          class="mt-0.5 size-7"
        />
        <div class="min-w-0 flex-1">
          <p class="flex flex-wrap items-baseline gap-x-2">
            <span class="font-medium">{author}</span>
            <time
              class="text-xs text-muted-foreground"
              datetime={new Date(comment.createAt).toISOString()}
              title={displayDateTime(comment.createAt, context)}
            >
              {relativeTime(comment.createAt, workspace.locale, Math.max(now, comment.createAt))}
            </time>
          </p>
          <BodyView doc={comment.body} {resolveUser} class="text-base">
            {#snippet invalid()}
              <p class="text-muted-foreground">{m.card_body_invalid()}</p>
            {/snippet}
          </BodyView>
        </div>
        {#if board.canComment && comment.createdBy === workspace.me?.id}
          <IconButton
            label={m.card_comment_delete_from({
              when: displayDateTime(comment.createAt, context),
            })}
            class="shrink-0 text-muted-foreground"
            onclick={() => void card.deleteComment(comment.id)}
          >
            <Trash2Icon aria-hidden="true" />
          </IconButton>
        {/if}
      </li>
    {/each}
  </ol>
</section>
