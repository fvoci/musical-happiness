<!-- A select option as it is shown everywhere: a rounded label on the option's colour. -->
<script lang="ts">
  import type { Option } from './data.ts';

  interface Props {
    option: Option;
  }

  let { option }: Props = $props();
</script>

<span
  class="inline-flex h-6 max-w-full shrink-0 items-center rounded-full px-2 text-xs font-medium whitespace-nowrap text-foreground"
  style:background-color="var(--chip-{option.hue})"
>
  {option.label}
</span>
