<!--
  What stands for a board where there is no room for its title, and beside
  the title where there is: its icon, or for a board without one the first
  character of its title in a small frame.

  A wiki is told apart from a board: one without an icon has a book in place
  of the character, and one with an icon a small book at its corner.

  Always decoration: the title is written next to it, or is the name of the
  button it sits in (which says that it is a wiki).
-->
<script lang="ts">
  import BookOpenIcon from '@lucide/svelte/icons/book-open';

  import { avatarInitial } from '$lib/components/avatar/avatar.ts';
  import type { ContainerKind } from '$lib/logic/wire.ts';

  interface Props {
    icon: string | undefined;
    title: string;
    /** The reader's language, for where a character ends and how it is uppercased. */
    locale: string;
    /** A board when left out. */
    kind?: ContainerKind | undefined;
  }

  let { icon, title, locale, kind }: Props = $props();

  const wiki = $derived(kind === 'wiki');
</script>

{#if icon}
  <span
    aria-hidden="true"
    data-board-icon="icon"
    data-kind={kind ?? 'board'}
    class="relative inline-grid size-5 shrink-0 place-items-center text-base leading-none"
  >
    {icon}
    {#if wiki}
      <BookOpenIcon
        class="absolute -right-1 -bottom-1 size-3 rounded-[3px] bg-sidebar p-px text-muted-foreground"
      />
    {/if}
  </span>
{:else if wiki}
  <span
    aria-hidden="true"
    data-board-icon="wiki"
    data-kind="wiki"
    class="inline-grid size-5 shrink-0 place-items-center rounded-sm border border-input-border text-muted-foreground"
  >
    <BookOpenIcon class="size-3.5" />
  </span>
{:else}
  <span
    aria-hidden="true"
    data-board-icon="initial"
    data-kind="board"
    class="inline-grid size-5 shrink-0 place-items-center rounded-sm border border-input-border text-xs leading-none font-medium text-muted-foreground"
  >
    {avatarInitial(title, locale)}
  </span>
{/if}
