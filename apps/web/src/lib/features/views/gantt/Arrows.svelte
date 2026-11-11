<!--
  The arrows of the chart: one drawing laid over the days, under the bars.
  Each arrow goes from the end of the bar of the card that comes first to
  the start of the bar of the card that comes after it.

  The drawing is as wide as the axis in rem and as high as the rows, and its
  own units are rem across and rows down (lib/logic/gantt.ts): it stretches
  to whatever height the style sheet gives a row. Lines keep their thickness
  whatever the stretch.

  An arrow between two cards whose dates are out of order is dashed, one
  that lies on a loop is dotted, and one that is both has a dot after each
  dash: the colour they also get is not the only sign. The row of the card says the same with a mark by its title, and its
  bar in words.

  For the eye only: a bar says to a screen reader which cards it comes after.
-->
<script lang="ts">
  import type { ArrowLine } from './chart.svelte.ts';

  interface Props {
    arrows: readonly ArrowLine[];
    /** The width of the axis in rem, and how many rows there are. */
    width: number;
    rows: number;
  }

  let { arrows, width, rows }: Props = $props();

  /** In pixels, as the thickness of the line is. A dot is a dash of no length with a round end. */
  const DASHED = '6 4';
  const DOTTED = '0.1 4.5';
  const DASH_DOT = '6 4.5 0.1 4.5';

  function dashes(arrow: ArrowLine): string | undefined {
    if (arrow.late) return arrow.loop ? DASH_DOT : DASHED;
    return arrow.loop ? DOTTED : undefined;
  }
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
    {#each arrows as arrow (arrow.key)}
      <g
        data-arrow
        data-from={arrow.from}
        data-to={arrow.to}
        data-late={arrow.late ? '' : undefined}
        data-loop={arrow.loop ? '' : undefined}
        class={[(arrow.late || arrow.loop) && 'text-destructive']}
      >
        <path
          d={arrow.path}
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linejoin="round"
          stroke-linecap={arrow.loop ? 'round' : undefined}
          stroke-dasharray={dashes(arrow)}
          vector-effect="non-scaling-stroke"
        />
        <path d={arrow.head} fill="currentColor" />
      </g>
    {/each}
  </svg>
{/if}
