<!--
  The comments of a card, oldest first: who wrote it, how long ago, and the
  text, drawn with the read-only view of a body. A comment somebody else
  writes while the card is open appears by itself (the card store follows
  the board's changes) and is announced.

  The reader can change the text of their own comments in place (the same
  plain field as a new comment: lines are paragraphs) and delete them;
  "Undo" in the card's menu brings a deleted one back. The field for a new comment is not here: it is pinned to the
  bottom edge of the card (CommentComposer).
-->
<script lang="ts">
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';
  import { onMount, tick } from 'svelte';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { isComposing } from '$lib/components/keys.ts';
  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { displayDateTime, relativeTime } from '$lib/logic/format.ts';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { autosize } from './autosize.ts';
  import { commentDoc, commentText } from './comment-doc.ts';

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

  /**
   * The comment whose text is being changed: the text as it is now, and the
   * version it was begun from. `theirs`: saving was refused because somebody
   * else changed the comment since; their text is shown, and saving again
   * writes over it.
   */
  let editing = $state<{ id: string; text: string; readAt: number; theirs: boolean } | undefined>();
  let saving = $state(false);
  const edited = $derived(editing ? commentDoc(editing.text) : null);
  /** The comment being changed, as it is stored now. */
  const current = $derived(editing ? card.comment(editing.id) : undefined);

  let list = $state<HTMLElement>();

  /** Closes the field and gives the focus back to the comment's edit button. */
  async function close(id: string): Promise<void> {
    if (editing?.id !== id) return;
    editing = undefined;
    await tick();
    list
      ?.querySelector<HTMLElement>(`[data-comment="${CSS.escape(id)}"] [data-action="edit"]`)
      ?.focus();
  }

  /** `editing` as it is now (read after a wait, when it may have changed). */
  const editingNow = (): typeof editing => editing;

  async function save(): Promise<void> {
    if (!editing || !edited || saving) return;
    const { id, readAt } = editing;
    saving = true;
    try {
      const outcome = await card.editComment(id, edited, readAt);
      if (outcome === 'saved') await close(id);
      else if (outcome === 'conflict') {
        // The field may have been closed, or another opened, while it was sent.
        const open = editingNow();
        if (open?.id === id) {
          // Shown now, theirs is what the next save knowingly writes over.
          open.theirs = true;
          open.readAt = card.comment(id)?.updateAt ?? readAt;
        }
      }
      // Failed: the field stays, with what was written.
    } finally {
      saving = false;
    }
  }

  function onkeydown(event: KeyboardEvent): void {
    if (isComposing(event)) return;
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void save();
    } else if (event.key === 'Escape' && editing) {
      // Leaves the field, not the card.
      event.preventDefault();
      event.stopPropagation();
      void close(editing.id);
    }
  }

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
  <ol
    bind:this={list}
    class="flex flex-col gap-4 empty:hidden"
    aria-live="polite"
    aria-relevant="additions"
  >
    {#each card.comments as comment (comment.id)}
      {@const author = workspace.userName(comment.createdBy) ?? m['centerPanel.unknown-user']()}
      {@const text = commentText(comment.body)}
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
          {#if editing?.id === comment.id}
            <form
              class="mt-1 flex flex-col gap-2"
              data-testid="comment-edit"
              onsubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              {#if editing.theirs}
                <div
                  class="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm"
                  role="status"
                  data-testid="comment-theirs"
                >
                  <p class="font-medium">{m.card_comment_conflict()}</p>
                  <BodyView doc={current?.body ?? null} {resolveUser} class="mt-1 text-sm" />
                </div>
              {/if}
              <!-- The field opens on the reader's own press, to be typed in. -->
              <!-- svelte-ignore a11y_autofocus -->
              <textarea
                bind:value={editing.text}
                rows="1"
                autofocus
                aria-label={m.card_comment_edit_field()}
                aria-keyshortcuts="Control+Enter Meta+Enter"
                class="field-sizing-content max-h-60 min-h-8 resize-none rounded-lg border border-input-border bg-transparent px-2.5 py-1 text-base outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"
                {onkeydown}
                {@attach autosize(() => editing?.text ?? '')}></textarea>
              <div class="flex justify-end gap-2">
                <Button type="button" variant="ghost" onclick={() => void close(comment.id)}>
                  {m.shared_cancel()}
                </Button>
                <Button type="submit" disabled={!edited || saving}>
                  {m.card_comment_save()}
                </Button>
              </div>
            </form>
          {:else}
            <BodyView doc={comment.body} {resolveUser} class="text-base">
              {#snippet invalid()}
                <p class="text-muted-foreground">{m.card_body_invalid()}</p>
              {/snippet}
            </BodyView>
          {/if}
        </div>
        {#if text !== undefined && editing?.id !== comment.id && board.canEditComment(comment.createdBy)}
          <IconButton
            label={m.card_comment_edit_from({
              when: displayDateTime(comment.createAt, context),
            })}
            class="shrink-0 text-muted-foreground"
            data-action="edit"
            onclick={() => {
              editing = { id: comment.id, text, readAt: comment.updateAt, theirs: false };
            }}
          >
            <PencilIcon aria-hidden="true" />
          </IconButton>
        {/if}
        {#if board.canDeleteComment(comment.createdBy)}
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
