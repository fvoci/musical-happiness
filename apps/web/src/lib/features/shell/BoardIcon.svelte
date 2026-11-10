<!--
  What stands for a board where there is no room for its title, and beside
  the title where there is: its icon, or for a board without one the first
  character of its title in a small frame.

  Always decoration: the title is written next to it, or is the name of the
  button it sits in.
-->
<script lang="ts">
  import { avatarInitial } from '$lib/components/avatar/avatar.ts';

  interface Props {
    icon: string | undefined;
    title: string;
    /** The reader's language, for where a character ends and how it is uppercased. */
    locale: string;
  }

  let { icon, title, locale }: Props = $props();
</script>

{#if icon}
  <span
    aria-hidden="true"
    data-board-icon="icon"
    class="inline-grid size-5 shrink-0 place-items-center text-base leading-none"
  >
    {icon}
  </span>
{:else}
  <span
    aria-hidden="true"
    data-board-icon="initial"
    class="inline-grid size-5 shrink-0 place-items-center rounded-sm border border-input-border text-xs leading-none font-medium text-muted-foreground"
  >
    {avatarInitial(title, locale)}
  </span>
{/if}
