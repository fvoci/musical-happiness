<!--
  What a comment said before it was changed, the text it was posted with
  first: who wrote each text and when. Only to read; nothing is put back
  from here. Read again when the comment is changed while it is open.
-->
<script lang="ts">
  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { displayDateTime, type DisplayContext } from '$lib/logic/format.ts';
  import type { BoardStore, CommentVersion } from '$lib/state/index.ts';

  interface Props {
    board: BoardStore;
    commentId: string;
    /** When the comment was last changed: a new change reads the texts again. */
    editedAt: number;
    context: DisplayContext;
    resolveUser: (userId: string) => string;
    id: string;
  }

  let { board, commentId, editedAt, context, resolveUser, id }: Props = $props();

  /** The texts as of the comment's last change: a newer one reads them again. */
  const versions = $derived(versionsAsOf(commentId, editedAt));

  function versionsAsOf(comment: string, _changedAt: number): Promise<CommentVersion[]> {
    return board.workspace.backend.commentVersions(board.boardId, comment);
  }
</script>

<div {id} class="mt-2 rounded-lg border bg-muted/40 px-3 py-2" data-testid="comment-history">
  {#await versions}
    <p class="text-sm text-muted-foreground">{m.app_loading()}</p>
  {:then list}
    <ol class="flex flex-col gap-3">
      {#each list as version (version.version)}
        {@const who = resolveUser(version.writtenBy)}
        {@const when = displayDateTime(version.writtenAt, context)}
        <li data-version={version.version}>
          <p class="text-xs text-muted-foreground">
            {version.version === 1
              ? m.card_comment_version_posted({ who, when })
              : m.card_comment_version_changed({ who, when })}
          </p>
          <BodyView doc={version.body} {resolveUser} class="text-sm">
            {#snippet invalid()}
              <p class="text-muted-foreground">{m.card_body_invalid()}</p>
            {/snippet}
          </BodyView>
        </li>
      {/each}
    </ol>
  {:catch}
    <p class="text-sm text-muted-foreground" role="status">
      {m.card_comment_history_failed()}
    </p>
  {/await}
</div>
