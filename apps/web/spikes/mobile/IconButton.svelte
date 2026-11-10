<!--
  A button that is only an icon. 32 pixels for a mouse; where the pointer is a
  finger it grows to the smallest side a finger can hit (`--spacing-touch`).
-->
<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  import { Button } from '$lib/ui/button/index.js';

  type Props = HTMLButtonAttributes & {
    /** What the button does, for a screen reader. */
    label: string;
  };

  let { label, class: className, children, ...rest }: Props = $props();

  // The generated button takes the attributes of a button and of a link at once;
  // those of a button alone do not fit its type.
  const passed = $derived<Record<string, unknown>>(rest);
</script>

<Button
  variant="ghost"
  size="icon"
  aria-label={label}
  class={['pointer-coarse:size-touch', className]}
  {...passed}
>
  {@render children?.()}
</Button>
