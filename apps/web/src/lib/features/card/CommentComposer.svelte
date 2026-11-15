<!--
  The field for a new comment: the last row of the card, always in sight
  (question 9 of docs/mobile-proposal.md). It stays above the on-screen
  keyboard because the card ends where the keyboard begins (frame.ts), and
  clear of the home indicator when no keyboard covers that.

  A plain text box of several lines, not the editor: Enter starts a line,
  Ctrl+Enter (⌘+Enter) or "Send" sends. Lines become paragraphs
  (comment-doc.ts). What was typed stays until it was stored; a comment that
  could not be stored is still in the field.

  A comment that is being written is not lost by leaving the card: Escape in
  the field only leaves the field (the next one closes the card), and a card
  that is closed with a draft has it again when it is opened later in the
  same visit (comment-drafts.ts).
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import { isComposing, primaryModifier, shortcutKeys, shortcutLabel } from '$lib/keys.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BoardStore, CardStore } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { autosize } from '$lib/components/autosize.ts';
  import { commentDoc } from './comment-doc.ts';
  import { draftOf, keepDraft } from './comment-drafts.ts';

  interface Props {
    board: BoardStore;
    card: CardStore;
    /** A comment was stored: the card shows it. */
    onSent?: () => void;
  }

  let { board, card, onSent }: Props = $props();

  const workspace = $derived(board.workspace);
  const me = $derived(workspace.me);
  const myName = $derived(me ? (workspace.userName(me.id) ?? me.username) : '');

  const cardId = untrack(() => card.cardId);
  let draft = $state(draftOf(cardId));
  let sending = $state(false);
  $effect(() => {
    keepDraft(cardId, draft);
  });
  const doc = $derived(commentDoc(draft));

  async function send(): Promise<void> {
    if (!doc || sending) return;
    sending = true;
    const sent = draft;
    const tell = onSent;
    try {
      const stored = await card.addComment(doc);
      if (stored) {
        // Typed on while it was being sent: that is the start of the next comment.
        if (draft.startsWith(sent)) draft = draft.slice(sent.length).trimStart();
        tell?.();
      }
    } finally {
      sending = false;
    }
  }

  function onkeydown(event: KeyboardEvent): void {
    if (isComposing(event)) return;
    if (event.key === 'Enter' && primaryModifier(event)) {
      event.preventDefault();
      void send();
    } else if (event.key === 'Escape' && draft.trim() !== '') {
      // With something written, Escape leaves the field and not the card.
      event.preventDefault();
      event.stopPropagation();
      (event.currentTarget as HTMLElement).blur();
    }
  }
</script>

<form
  class="flex shrink-0 items-end gap-2 border-t bg-inherit px-3 pt-2 pb-[calc(0.5rem+max(0px,var(--safe-bottom)-var(--keyboard)))] @card-roomy/card:px-6"
  data-testid="comment-composer"
  onsubmit={(event) => {
    event.preventDefault();
    void send();
  }}
>
  {#if me}
    <Avatar
      name={myName}
      colorKey={me.id}
      locale={workspace.locale}
      decorative
      class="mb-0.5 size-7 roomy:mb-2"
    />
  {/if}
  <textarea
    bind:value={draft}
    rows="1"
    aria-label={m['CardDetail.new-comment-placeholder']()}
    aria-keyshortcuts={shortcutKeys('Enter')}
    title={m.card_comment_send_hint({ shortcut: shortcutLabel('Enter') })}
    placeholder={m['CardDetail.new-comment-placeholder']()}
    data-testid="comment-field"
    class="field-sizing-content max-h-40 min-h-8 min-w-0 flex-1 resize-none rounded-lg border border-input-border bg-transparent px-2.5 py-1 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30 roomy:min-h-touch roomy:py-2 roomy:text-base"
    {onkeydown}
    {@attach autosize(() => draft)}></textarea>
  <Button
    type="submit"
    class="roomy:min-h-touch"
    disabled={!doc || sending}
    data-testid="comment-send"
  >
    {m['CommentsList.send']()}
  </Button>
</form>
