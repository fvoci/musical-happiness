<!--
  The ★ of a board, a card, a page or a unit, as the reader's stars answer it:
  not starred, starred, and a press that switches it; nothing until the stars
  are read.
-->
<script lang="ts">
  import FavoriteToggle from '$lib/components/favorite-toggle/FavoriteToggle.svelte';
  import type { ReaderStars } from '$lib/components/favorite-toggle/reader-stars.ts';
  import { SvelteSet } from 'svelte/reactivity';

  let { unread = false }: { unread?: boolean } = $props();

  const starred = new SvelteSet<string>(['starred']);

  const stars: ReaderStars = {
    has: (targetId) => (unread ? undefined : starred.has(targetId)),
    read: () => Promise.resolve(),
    toggle: (targetId) => {
      if (starred.has(targetId)) starred.delete(targetId);
      else starred.add(targetId);
      return Promise.resolve();
    },
  };
</script>

<div class="flex items-center gap-3" data-testid="favorite-samples">
  <FavoriteToggle {stars} targetId="plain" />
  <FavoriteToggle {stars} targetId="starred" />
</div>
