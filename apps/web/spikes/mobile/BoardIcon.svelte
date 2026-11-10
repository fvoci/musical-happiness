<!--
  Stands in for the icon of a board. The real one is an emoji; the machine the
  screenshots are taken on has no emoji font, so this is a coloured square
  with the first character of the name.
-->
<script lang="ts">
  import type { BoardLink } from './data.ts';

  interface Props {
    board: BoardLink;
  }

  let { board }: Props = $props();
</script>

<span
  aria-hidden="true"
  class="inline-flex size-5 shrink-0 items-center justify-center rounded-sm text-xs font-semibold text-foreground"
  style:background-color="var(--chip-{board.hue})"
>
  {board.name.slice(0, 1)}
</span>
