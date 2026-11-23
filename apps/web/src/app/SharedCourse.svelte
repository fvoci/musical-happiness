<!--
  A course read by its public link (`/shared/<course>/<unit>?r=<token>`, the
  unit in the place a board's view has): its chapters and units on the left,
  and the unit the address names (the first without one) as read-only content.
  Nobody is named, there are no comments, no versions, and nothing changes.
  Only references to other live units of the same course open; everything else
  is left unseen.
-->
<script lang="ts">
  import ListTreeIcon from '@lucide/svelte/icons/list-tree';

  import BodyView from '$lib/editor/BodyView.svelte';
  import type { ReferenceDrawing } from '$lib/editor/reference-chip.ts';
  import OutlineButton from '$lib/features/wiki/OutlineButton.svelte';
  import OutlineColumns from '$lib/features/wiki/OutlineColumns.svelte';
  import { PageOutline } from '$lib/features/wiki/page-outline.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { pane } from '$lib/layout/room.svelte.ts';
  import { firstUnit, pageRows, pageTree } from '$lib/logic/page-tree.ts';
  import {
    isBackendError,
    type Backend,
    type CourseUnit,
    type CourseUnitDetail,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  interface Props {
    backend: Backend;
    courseId: string;
    router: Router;
  }

  let { backend, courseId, router }: Props = $props();

  let status = $state<'loading' | 'ready' | 'failed'>('loading');
  let units = $state.raw<CourseUnit[]>([]);
  /** The unit shown, read with its body; `null` for one that is not there. */
  let shown = $state.raw<CourseUnitDetail | null | undefined>(undefined);
  let unitFailed = $state(false);
  let treeOpen = $state(false);
  let treeReadings = 0;
  let unitReadings = 0;
  const uid = $props.id();

  const unitId = $derived(router.route.viewId);
  const rows = $derived(pageRows(pageTree(units), () => true));
  const hasUnits = $derived(units.some((item) => item.kind !== 'chapter'));
  const narrow = $derived(pane.measured && !pane.columns);
  const outline = new PageOutline();
  const hrefOf = (id: string): string =>
    router.href({ ...router.route, viewId: id, cardId: undefined });

  const references: ReferenceDrawing = {
    shown: (id) => {
      const unit = units.find((one) => one.id === id && one.kind === 'unit');
      return unit ? { title: unit.title, icon: '', kind: 'unit', href: hrefOf(id) } : null;
    },
    open: (id) => {
      if (units.some((one) => one.id === id && one.kind === 'unit')) {
        router.go({ ...router.route, viewId: id, cardId: undefined });
      }
    },
  };

  async function readTree(): Promise<void> {
    const courses = backend.course;
    if (!courses) return;
    treeReadings += 1;
    const reading = treeReadings;
    status = 'loading';
    try {
      const read = await courses.units(courseId);
      if (reading !== treeReadings) return;
      [units, status] = [read, 'ready'];
    } catch {
      if (reading === treeReadings) status = 'failed';
    }
  }

  async function readUnit(id: string): Promise<void> {
    const courses = backend.course;
    if (!courses) return;
    unitReadings += 1;
    const reading = unitReadings;
    [shown, unitFailed] = [undefined, false];
    try {
      const unit = await courses.unit(courseId, id);
      if (reading !== unitReadings) return;
      shown = unit.kind === 'chapter' ? null : unit;
    } catch (error) {
      if (reading !== unitReadings) return;
      if (isBackendError(error) && error.status === 404) shown = null;
      else unitFailed = true;
    }
  }

  $effect(() => {
    void readTree();
  });

  // Without a unit in the address (or if it names a chapter): open the first unit.
  $effect(() => {
    if (status !== 'ready') return;
    if (!unitId) {
      const first = firstUnit(units);
      if (first) router.go({ ...router.route, viewId: first.id }, { replace: true });
      return;
    }
    const node = units.find((item) => item.id === unitId);
    if (node?.kind === 'chapter') {
      const child = firstUnit(units.filter((item) => item.parentId === node.id));
      const fallback = child ?? firstUnit(units);
      if (fallback) router.go({ ...router.route, viewId: fallback.id }, { replace: true });
    }
  });

  $effect(() => {
    if (status !== 'ready') return;
    const id = unitId;
    if (!id || units.find((item) => item.id === id)?.kind === 'chapter') return;
    void readUnit(id);
  });

  $effect(() => {
    backend.presence.setPresence({ workspaceId: '', containerId: courseId, cardId: unitId ?? '' });
  });
  $effect(() => () => {
    backend.presence.setPresence(null);
  });
</script>

{#snippet problem(retry: () => void)}
  <div class="grid h-full place-items-center p-6 text-center">
    <div>
      <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={retry}>{m.app_retry()}</Button>
    </div>
  </div>
{/snippet}

{#snippet tree()}
  <nav
    id="{uid}-tree"
    aria-label={m.course_units()}
    class={[narrow && 'max-h-[50dvh] overflow-y-auto overscroll-contain']}
    data-testid="shared-unit-tree"
  >
    <ul class="grid gap-0.5 p-2">
      {#each rows as row (row.page.id)}
        <li style:padding-left="{row.depth * 0.75}rem">
          {#if row.page.kind === 'chapter'}
            <span
              class="flex min-h-7 items-center truncate px-2 pt-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase"
              data-kind="chapter"
            >
              {row.page.title || m.course_chapter_untitled()}
            </span>
          {:else}
            <a
              href={hrefOf(row.page.id)}
              aria-current={row.page.id === unitId ? 'page' : undefined}
              class="flex min-h-8 items-center gap-1.5 truncate rounded-md px-2 hover:bg-muted aria-[current=page]:bg-muted aria-[current=page]:font-medium roomy:min-h-touch"
              onclick={(event) => {
                if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
                event.preventDefault();
                treeOpen = false;
                router.go({ ...router.route, viewId: row.page.id, cardId: undefined });
              }}
            >
              <span class="truncate">{row.page.title || m.course_unit_untitled()}</span>
            </a>
          {/if}
        </li>
      {/each}
    </ul>
  </nav>
{/snippet}

{#if status === 'loading'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if status === 'failed'}
  {@render problem(() => void readTree())}
{:else}
  <div class={['flex min-h-0 flex-1', narrow && 'flex-col']} data-testid="shared-course">
    {#if narrow}
      <div class="shrink-0 border-b">
        <div class="flex items-center">
          <button
            type="button"
            class="flex min-h-touch min-w-0 flex-1 items-center gap-2 px-4 text-sm text-muted-foreground"
            aria-expanded={treeOpen}
            aria-controls="{uid}-tree"
            onclick={() => {
              treeOpen = !treeOpen;
            }}
          >
            <ListTreeIcon class="size-4" aria-hidden="true" />
            {m.course_units()}
          </button>
          {#if outline.mode === 'button'}
            <div class="shrink-0 pr-2"><OutlineButton {outline} id="{uid}-outline" /></div>
          {/if}
        </div>
        {#if treeOpen}{@render tree()}{/if}
      </div>
    {:else}
      <div class="w-64 shrink-0 overflow-y-auto border-r">{@render tree()}</div>
    {/if}
    <article
      class="min-w-0 flex-1 overflow-y-auto px-4"
      data-testid="shared-unit"
      {@attach outline.follow}
    >
      {#if !narrow && outline.mode === 'button'}
        <div
          class="sticky top-0 z-10 -mx-4 flex justify-end bg-background/95 px-2 py-1"
          {@attach outline.bar}
        >
          <OutlineButton {outline} id="{uid}-outline" />
        </div>
      {/if}
      {#if !hasUnits}
        <p class="py-6 text-muted-foreground">{m.course_no_units()}</p>
      {:else if shown === null}
        <NotFound {router} />
      {:else if unitFailed && unitId}
        {@const id = unitId}
        {@render problem(() => void readUnit(id))}
      {:else if shown}
        <OutlineColumns {outline} id="{uid}-outline">
          <div class="mx-auto max-w-3xl py-6">
            <h2 class="mb-4 text-2xl font-semibold break-words" data-testid="unit-title">
              {shown.title || m.course_unit_untitled()}
            </h2>
            <div data-testid="unit-body" {@attach outline.source}>
              <BodyView doc={shown.body} {references}>
                {#snippet empty()}
                  <p class="text-muted-foreground">{m.course_unit_body_empty()}</p>
                {/snippet}
              </BodyView>
            </div>
          </div>
        </OutlineColumns>
      {:else}
        <p class="py-6 text-muted-foreground" role="status">{m.app_loading()}</p>
      {/if}
    </article>
  </div>
{/if}
