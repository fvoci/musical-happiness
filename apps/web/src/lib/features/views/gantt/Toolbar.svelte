<!--
  The row above the chart: "Today" brings today into view.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    ontoday: () => void;
  }

  let { ontoday }: Props = $props();
</script>

<div
  data-testid="gantt-toolbar"
  class="flex h-12 shrink-0 items-center gap-1 border-b px-1.5 @columns/pane:px-3"
>
  <Button
    variant="outline"
    data-testid="gantt-today"
    class="roomy:h-touch roomy:px-3"
    onclick={ontoday}
  >
    {m.calendar_today()}
  </Button>
  <div class="min-w-1 flex-1"></div>
</div>
