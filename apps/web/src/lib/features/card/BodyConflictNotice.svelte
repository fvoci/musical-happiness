<!--
  Shown above the body when the reader's writing met somebody else's: the
  write was refused and nothing was overwritten.

  It does not say who wrote the other text: the refused write does not tell.
  The reader chooses between the two, and can look at the stored one first.
  Until they choose, their own text stays in the editor as they left it.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import BodyView from '$lib/editor/BodyView.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { BodyConflict } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import type { BodySaver } from './body-saver.svelte.ts';

  interface Props {
    conflict: BodyConflict;
    saver: BodySaver;
    resolveUser: (userId: string) => string | undefined;
  }

  let { conflict, saver, resolveUser }: Props = $props();

  const uid = $props.id();
  let notice = $state<HTMLElement>();
  /** A choice was made and is on its way; the buttons wait. */
  let settling = $state(false);

  async function settle(keep: 'mine' | 'theirs'): Promise<void> {
    settling = true;
    try {
      if (keep === 'mine') await saver.keepMine();
      else await saver.takeTheirs();
    } finally {
      settling = false;
    }
  }

  // The reader may be far down the card, in the comments: bring the notice to them.
  onMount(() => {
    notice?.scrollIntoView({ block: 'nearest' });
  });
</script>

<div
  bind:this={notice}
  class="flex flex-col gap-3 rounded-lg border border-destructive/40 bg-destructive/10 p-3"
  role="alert"
  data-testid="body-conflict"
>
  <div>
    <p class="font-medium">{m.card_conflict_title()}</p>
    <p>{m.card_conflict_text()}</p>
  </div>
  <details class="rounded-md border bg-background">
    <summary
      class="flex min-h-8 cursor-pointer items-center px-3 font-medium roomy:min-h-touch"
      data-testid="conflict-preview"
    >
      {m.card_conflict_preview()}
    </summary>
    <div class="border-t px-3 py-2" data-testid="conflict-theirs">
      <BodyView doc={conflict.theirs.body} {resolveUser} class="text-base">
        {#snippet empty()}
          <p class="text-muted-foreground">{m.card_body_empty()}</p>
        {/snippet}
        {#snippet invalid()}
          <p class="text-muted-foreground">{m.card_body_invalid()}</p>
        {/snippet}
      </BodyView>
    </div>
  </details>
  <div class="flex flex-wrap items-center gap-2">
    <Button
      class="roomy:min-h-touch"
      disabled={settling}
      data-testid="conflict-mine"
      onclick={() => void settle('mine')}
    >
      {m.card_conflict_mine()}
    </Button>
    <Button
      variant="outline"
      class="roomy:min-h-touch"
      disabled={settling}
      data-testid="conflict-theirs-button"
      aria-describedby="{uid}-hint"
      onclick={() => void settle('theirs')}
    >
      {m.card_conflict_theirs()}
    </Button>
  </div>
  <p id="{uid}-hint" class="text-xs text-muted-foreground">
    {m.card_conflict_theirs_hint()}
  </p>
</div>
