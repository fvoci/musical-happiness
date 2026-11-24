<!--
  The content column of an open course unit: its collaborative body (with `[[`
  references and ⌘S manual versions), backlinks ("What links to this unit"),
  and comments (read-only for unjoined readers of an open course).
-->
<script lang="ts">
  import type { Attachment } from 'svelte/attachments';

  import { bodyReferences } from '$lib/features/card/body-references.ts';
  import { keepVersion, unitVersions } from '$lib/features/card/body-versions.svelte.ts';
  import CardBacklinks from '$lib/features/card/CardBacklinks.svelte';
  import CardBody from '$lib/features/card/CardBody.svelte';
  import CardComments from '$lib/features/card/CardComments.svelte';
  import CommentComposer from '$lib/features/card/CommentComposer.svelte';
  import UnitNotesPanel from '$lib/features/course/UnitNotesPanel.svelte';
  import UnitReviewsPanel from '$lib/features/course/UnitReviewsPanel.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { CourseStore, UnitStore, Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    workspace: Workspace;
    store: CourseStore;
    unit: UnitStore;
    outlined: Attachment<HTMLElement>;
  }

  let { workspace, store, unit, outlined }: Props = $props();
</script>

<div {@attach outlined}>
  <CardBody
    container={store}
    card={unit}
    of="unit"
    testId="unit-body"
    references={bodyReferences(store, unit.unitId)}
    keep={() => keepVersion(unitVersions(unit))}
  />
</div>
{#if unit.status === 'ready'}
  <UnitReviewsPanel course={store} unitId={unit.unitId} />
  <UnitNotesPanel course={store} unitId={unit.unitId} />
{/if}
<CardBacklinks {workspace} blockId={unit.unitId} of="unit" heading="h2" />
{#if unit.status === 'ready' && unit.comments.status === 'ready'}
  <CardComments
    board={unit.comments}
    card={unit.comments}
    readVersions={(commentId: string) => unit.comments.commentVersions(commentId)}
    heading="h2"
  />
  {#if unit.comments.canComment}
    <CommentComposer
      board={unit.comments}
      card={unit.comments}
      draftKey={unit.unitId}
      placement="inline"
    />
  {/if}
{:else if unit.comments.status === 'failed'}
  <div class="flex flex-wrap items-center gap-2 border-t pt-5" role="alert">
    <p class="text-muted-foreground">{m.app_problem_load()}</p>
    <Button variant="outline" onclick={() => void unit.comments.read(false)}>
      {m.app_retry()}
    </Button>
  </div>
{/if}
