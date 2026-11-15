<!--
  The bar above an open wiki the reader is no member of: they read it as it
  is, and join it here to write in it.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    joining: boolean;
    /** Joins; the wiki is then read again, for writing. */
    onJoin: () => void;
  }

  let { joining, onJoin }: Props = $props();
</script>

<div
  class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-b bg-muted px-3 py-2"
  role="region"
  aria-label={m.wiki_join()}
  data-testid="wiki-join"
>
  <p class="min-w-0 flex-1 text-sm text-muted-foreground">{m.wiki_join_hint()}</p>
  <Button
    size="sm"
    class="roomy:min-h-touch"
    aria-disabled={joining}
    aria-busy={joining}
    onclick={() => {
      if (!joining) onJoin();
    }}>{m.wiki_join()}</Button
  >
</div>
