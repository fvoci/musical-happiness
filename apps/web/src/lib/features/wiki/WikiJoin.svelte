<!--
  An open wiki the reader is no member of: its title and description, and a
  way to join it, as an open board shows (the board's join screen). Its pages
  are not read until then.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Board } from '$lib/logic/wire.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    wiki: Board;
    joining: boolean;
    /** Joins; true once the wiki is read. */
    onJoin: () => Promise<boolean | undefined> | undefined;
  }

  let { wiki, joining, onJoin }: Props = $props();

  async function join(): Promise<void> {
    if (!(await onJoin())) return;
    await tick();
    document.querySelector<HTMLElement>('[data-testid="wiki-screen"]')?.focus();
  }
</script>

<div class="grid h-full place-items-center p-6 text-center" data-testid="wiki-join">
  <div class="flex max-w-md flex-col items-center gap-3">
    <h2 class="text-lg font-medium">
      {wiki.icon ? `${wiki.icon} ` : ''}{wiki.title || m.wiki_untitled()}
    </h2>
    {#if wiki.description}
      <p class="whitespace-pre-wrap text-muted-foreground">{wiki.description}</p>
    {/if}
    <p class="text-muted-foreground">{m.wiki_join_hint()}</p>
    <Button aria-disabled={joining} aria-busy={joining} onclick={() => void join()}
      >{m.wiki_join()}</Button
    >
  </div>
</div>
