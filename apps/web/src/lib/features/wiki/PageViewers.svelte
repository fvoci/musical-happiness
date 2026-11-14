<!--
  Who else has a page of the wiki open now: their faces, two at most and how
  many more, with their names for a screen reader. Nothing while nobody else
  has it. Small on a row of the tree, larger at the head of the page.
-->
<script lang="ts">
  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { WikiStore } from '$lib/state/index.ts';

  let {
    wiki,
    pageId,
    large = false,
    class: className,
  }: { wiki: WikiStore; pageId: string; large?: boolean; class?: string } = $props();

  const people = $derived(wiki.presence.others.filter((person) => person.cardIds.includes(pageId)));
  const nameOf = (userId: string): string =>
    wiki.workspace.userName(userId) ?? m['centerPanel.unknown-user']();
  const label = $derived(
    m.wiki_page_viewers({
      count: people.length,
      names: people.map((person) => nameOf(person.userId)).join(', '),
    }),
  );
</script>

{#if people.length > 0}
  <span
    data-testid="page-viewers"
    role="img"
    aria-label={label}
    title={label}
    class={['flex shrink-0 items-center gap-1 text-xs text-muted-foreground', className]}
  >
    <span class="flex -space-x-1">
      {#each people.slice(0, 2) as person (person.userId)}
        <Avatar
          name={nameOf(person.userId)}
          colorKey={person.userId}
          locale={wiki.workspace.locale}
          decorative
          class={large ? 'size-7 ring-1 ring-background' : 'size-4 ring-1 ring-background'}
        />
      {/each}
    </span>
    {#if people.length > 2}<span aria-hidden="true">+{people.length - 2}</span>{/if}
  </span>
{/if}
