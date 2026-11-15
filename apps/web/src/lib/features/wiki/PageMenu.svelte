<!--
  The menu at the head of a page: what can be done with the page shown. For
  now its versions, which whoever reads the wiki reads (`onVersions`).
-->
<script lang="ts">
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import HistoryIcon from '@lucide/svelte/icons/history';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { WikiPage } from '$lib/state/index.ts';

  interface Props {
    page: WikiPage;
    /** Opens the versions kept of the page's body. */
    onVersions: () => void;
  }

  let { page, onVersions }: Props = $props();

  const title = $derived(page.title || m.wiki_page_untitled());
  const actions = $derived<MenuAction[]>([
    { id: 'versions', label: m.card_versions(), icon: HistoryIcon, run: onVersions },
  ]);
</script>

<ActionMenu title={m.wiki_page_more({ title })} subtitle={title} align="end" {actions}>
  {#snippet trigger(props: Record<string, unknown>)}
    <IconButton {...props} label={m.wiki_page_more({ title })} data-testid="page-menu">
      <EllipsisIcon aria-hidden="true" />
    </IconButton>
  {/snippet}
</ActionMenu>
