<!--
  The arrows of the chart: one drawing laid over the days, under the bars.
  Each arrow goes from the end of the bar of the card that comes first to
  the start of the bar of the card that comes after it.

  The drawing is as wide as the axis in rem and as high as the rows, and its
  own units are rem across and rows down (lib/logic/gantt.ts): it stretches
  to whatever height the style sheet gives a row. Lines keep their thickness
  whatever the stretch.

  An arrow between two cards whose dates are out of order is dashed, and one
  that lies on a loop is dotted: the colour they also get is not the only
  sign. The bar of the card says the same with a mark and in words.

  For the eye only: a bar says to a screen reader which cards it comes after.
-->
<script lang="ts">
  import { arrowHead, arrowPath, type Arrow } from '$lib/logic/gantt.ts';

  interface Props {
    arrows: readonly Arrow[];
    /** The width of the axis in rem, and how many rows there are. */
    width: number;
    rows: number;
  }

  let { arrows, width, rows }: Props = $props();

  /** In pixels, as the thickness of the line is. */
  const DASHED = '6 4';
  const DOTTED = '0.1 4.5';
</script>

{#if arrows.length > 0 && rows > 0}
  <svg
    aria-hidden="true"
    data-testid="gantt-arrows"
    class="pointer-events-none absolute top-0 left-(--gantt-title) w-(--gantt-width) text-muted-foreground"
    style:height="calc(var(--gantt-row) * {rows})"
    viewBox="0 0 {width} {rows}"
    preserveAspectRatio="none"
  >
    {#each arrows as arrow (`${arrow.from}\u0000${arrow.to}`)}
      <g
        data-arrow
        data-from={arrow.from}
        data-to={arrow.to}
        data-late={arrow.late ? '' : undefined}
        data-loop={arrow.loop ? '' : undefined}
        class={[(arrow.late || arrow.loop) && 'text-destructive']}
      >
        <path
          d={arrowPath(arrow.points)}
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linejoin="round"
          stroke-linecap={arrow.loop && !arrow.late ? 'round' : undefined}
          stroke-dasharray={arrow.late ? DASHED : arrow.loop ? DOTTED : undefined}
          vector-effect="non-scaling-stroke"
        />
        <path d={arrowHead(arrow.points)} fill="currentColor" />
      </g>
    {/each}
  </svg>
{/if}
