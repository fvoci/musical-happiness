<!--
  The menu at the head of a unit: what can be done with the unit shown.
  Opens the versions kept of the unit's body (`onVersions`).
-->
<script lang="ts">
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import HistoryIcon from '@lucide/svelte/icons/history';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import type { MenuAction } from '$lib/components/menu/actions.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { CourseUnit } from '$lib/state/index.ts';

  interface Props {
    unit: CourseUnit;
    /** Opens the versions kept of the unit's body. */
    onVersions: () => void;
  }

  let { unit, onVersions }: Props = $props();

  const title = $derived(unit.title || m.course_unit_untitled());
  const actions = $derived<MenuAction[]>([
    { id: 'versions', label: m.card_versions(), icon: HistoryIcon, run: onVersions },
  ]);
</script>

<ActionMenu title={m.course_unit_more({ title })} subtitle={title} align="end" {actions}>
  {#snippet trigger(props: Record<string, unknown>)}
    <IconButton {...props} label={m.course_unit_more({ title })} data-testid="unit-menu">
      <EllipsisIcon aria-hidden="true" />
    </IconButton>
  {/snippet}
</ActionMenu>
