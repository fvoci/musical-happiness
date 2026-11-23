<!--
  One course (`/w/<workspace>/courses/<course>/<unit>`): its chapters and units
  as a depth-2 tree on the left, and the open unit's title, collaborative body,
  backlinks, comments, and body versions in the center. Composes
  `features/course/`, `features/wiki/` (`PageTree`, `MovePageDialog`,
  `PageViewers`, `OutlineButton`, `OutlineColumns`), `features/card/`
  (`CardBody`, `CardBacklinks`, `CardComments`, `CommentComposer`,
  `BodyVersions`), and `features/shell/` (`TrashDialog`, `ShareDialog`).
-->
<script lang="ts">
  import { untrack, type Snippet } from 'svelte';

  import CircleIcon from '@lucide/svelte/icons/circle';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';

  import { holdSaveKey } from '$lib/editor/save-key.ts';
  import BodyVersions from '$lib/features/card/BodyVersions.svelte';
  import { unitVersions, VersionsReading } from '$lib/features/card/body-versions.svelte.ts';
  import CourseJoin from '$lib/features/course/CourseJoin.svelte';
  import CourseProgressBar from '$lib/features/course/CourseProgressBar.svelte';
  import CourseScreen from '$lib/features/course/CourseScreen.svelte';
  import UnitMenu from '$lib/features/course/UnitMenu.svelte';
  import OutlineButton from '$lib/features/wiki/OutlineButton.svelte';
  import OutlineColumns from '$lib/features/wiki/OutlineColumns.svelte';
  import { PageOutline } from '$lib/features/wiki/page-outline.svelte.ts';
  import PageTree from '$lib/features/wiki/PageTree.svelte';
  import PageViewers from '$lib/features/wiki/PageViewers.svelte';
  import { COURSE_TREE_LABELS } from '$lib/features/wiki/tree-actions.ts';
  import { TreeOpen } from '$lib/features/wiki/tree-open.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { courseTreeRules, type PagePlace } from '$lib/logic/page-moves.ts';
  import { ancestorsOf, firstUnit } from '$lib/logic/page-tree.ts';
  import {
    CourseStore,
    referenceTitles,
    shownCourse,
    unitTrash,
    whileHere,
    type CourseUnit,
    type PageDraft,
    type Trash,
    type UnitStore,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import CourseDialogs from './CourseDialogs.svelte';
  import CourseUnitContent from './CourseUnitContent.svelte';
  import ManagedContainer from './ManagedContainer.svelte';
  import NotFound from './NotFound.svelte';
  import { referencePlaces } from './reference-places.ts';
  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  $effect.pre(() => {
    referenceTitles(workspace).places = referencePlaces(router);
  });

  const uid = $props.id();
  const courseId = $derived(router.route.courseId);
  const unitId = $derived(router.route.unitId);

  const course = $derived(courseId ? new CourseStore(workspace, courseId) : undefined);
  const rules = courseTreeRules<CourseUnit>();

  $effect(() => {
    const store = course;
    if (!store) return;
    const holder = shownCourse(workspace);
    untrack(() => {
      workspace.closeBoard();
      holder.current = store;
      notice = undefined;
      joined = false;
      made = undefined;
      [trash, trashOpen, sharing] = [undefined, false, false];
      [versions, versionsOpen] = [undefined, false];
      [deleteOpen, movingOpen, chapterOpen, settingsOpen, deleteCourseOpen] = [
        false,
        false,
        false,
        false,
        false,
      ];
      void store.load();
    });
    return () => {
      store.close();
      if (holder.current === store) holder.current = undefined;
    };
  });

  // Without a unit in the address (or if it points at a chapter): open the first unit.
  $effect(() => {
    if (course?.status !== 'ready') return;
    if (!unitId) {
      const first = firstUnit(course.units);
      if (first) router.openUnit(first.id, { replace: true });
      return;
    }
    const node = course.unit(unitId);
    if (node?.kind === 'chapter') {
      const child = firstUnit(course.units.filter((u) => u.parentId === node.id));
      const fallback = child ?? firstUnit(course.units);
      router.openUnit(fallback?.id, { replace: true });
    }
  });

  let joined = $state(false);
  let notice = $state<string | undefined>(undefined);
  let made = $state<string | undefined>(undefined);
  let creating = $state(false);

  const open = $derived(
    course
      ? new TreeOpen(workspace.me?.id ?? '', course.courseId, 'fvoci:course-open:')
      : undefined,
  );
  const outline = new PageOutline();

  // Keep ancestors of the open unit unfolded in the outline tree.
  const above = $derived(course && unitId ? ancestorsOf(course.units, unitId).join(' ') : '');
  $effect(() => {
    const ids = above;
    const target = open;
    if (ids !== '' && target) {
      untrack(() => {
        target.set(ids.split(' '));
      });
    }
  });

  // When the shown unit is deleted, move to the first remaining unit (or empty state).
  $effect(() => {
    const removed = course?.openUnit?.removed;
    if (!course || !removed) return;
    if (removed.by === 'others') {
      notice = m.course_unit_gone({ title: removed.title || m.course_unit_untitled() });
    }
    router.openUnit(firstUnit(course.units)?.id, { replace: true });
  });

  $effect(() => {
    course?.presence.setCard(unitId);
  });

  const placed = $derived(unitId !== undefined || !course || firstUnit(course.units) === undefined);

  $effect(() => {
    const shown = course;
    if (shown?.status === 'ready' && !shown.missing && placed) {
      return untrack(() => shown.presence.start());
    }
    return undefined;
  });

  $effect(() => {
    const shown = course;
    const id = unitId;
    if (shown?.status !== 'ready') return;
    const target = id && shown.unit(id)?.kind === 'chapter' ? undefined : id;
    untrack(() => {
      if (versions && versions.unitId !== id) [versions, versionsOpen] = [undefined, false];
      shown.showUnit(target);
    });
  });

  let deleting = $state.raw<CourseUnit | undefined>(undefined);
  let deleteOpen = $state(false);
  let moving = $state<string | undefined>(undefined);
  let movingOpen = $state(false);
  let chapterDialog = $state.raw<{ mode: 'create' } | { mode: 'rename'; chapter: CourseUnit }>({
    mode: 'create',
  });
  let chapterOpen = $state(false);
  let settingsOpen = $state(false);
  let sharing = $state(false);
  let deleteCourseOpen = $state(false);
  let versions = $state.raw<{ unitId: string; reading: VersionsReading }>();
  let versionsOpen = $state(false);
  let trash = $state.raw<Trash | undefined>(undefined);
  let trashOpen = $state(false);

  const hasUnits = $derived(course?.units.some((item) => item.kind !== 'chapter') ?? false);
  const reading = $derived(
    Boolean(
      course &&
      hasUnits &&
      !course.openUnit?.missing &&
      unitId &&
      course.unit(unitId)?.kind === 'unit',
    ),
  );

  async function createUnit(
    store: CourseStore,
    draft: PageDraft,
    onCreated?: () => void,
  ): Promise<void> {
    if (creating) return;
    creating = true;
    try {
      const created = await whileHere(store.workspace, () =>
        store.createUnit({
          kind: 'unit',
          ...(draft.title === undefined ? {} : { title: draft.title }),
          ...(draft.parentId === undefined ? {} : { parentId: draft.parentId }),
          ...(draft.after === undefined ? {} : { after: draft.after }),
        }),
      );
      if (!created || course !== store) return;
      if (created.parentId) open?.set([created.parentId]);
      onCreated?.();
      made = undefined;
      router.openUnit(created.id);
      made = created.id;
    } finally {
      creating = false;
    }
  }

  async function submitChapter(store: CourseStore, title: string): Promise<void> {
    if (chapterDialog.mode === 'rename') {
      await store.changeUnitTitle(chapterDialog.chapter.id, title);
      return;
    }
    if (creating) return;
    creating = true;
    try {
      const created = await whileHere(store.workspace, () =>
        store.createUnit({ kind: 'chapter', title }),
      );
      if (created && course === store) open?.set([created.id]);
    } finally {
      creating = false;
    }
  }

  function moveUnit(store: CourseStore, id: string, to: PagePlace): Promise<boolean> {
    if (to.parentId !== '') open?.set([to.parentId, ...ancestorsOf(store.units, to.parentId)]);
    return store.moveUnit(id, to);
  }

  async function deleteCourse(store: CourseStore): Promise<void> {
    const done = await whileHere(workspace, async () => {
      await workspace.deleteContainer(store.courseId);
      return true;
    });
    if (!done || workspace.containers.some((left) => left.id === store.courseId)) return;
    if (router.route.courseId !== store.courseId) return;
    const next = workspace.sidebar.flatMap((entry) => entry.containers)[0];
    if (next) router.openContainer(next);
    else router.go({ workspaceId: workspace.info?.id }, { replace: true });
  }
</script>

<!-- A unit shown, the keys that keep a version of its body never open the browser save dialog. -->
<svelte:window
  onkeydown={(event: KeyboardEvent) => {
    if (course?.openUnit) holdSaveKey(event);
  }}
/>

{#if !course || course.status === 'idle' || course.status === 'loading'}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if course.missing}
  {@const store = course}
  <ManagedContainer
    {workspace}
    {store}
    kind="course"
    shown={() => shownCourse(workspace).current === store}
    {router}
    onJoined={() => {
      joined = true;
    }}
  />
{:else if course.status === 'failed'}
  <div class="grid h-full place-items-center p-6 text-center">
    <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
  </div>
{:else if open}
  {@const store = course}
  {@const treeOpen = open}
  <div class="flex h-full min-h-0 flex-col">
    {#if store.joinable}
      <CourseJoin
        joining={store.joining}
        role={store.course?.minimumRole}
        onJoin={() => {
          void store.join().then((done) => {
            joined = done;
          });
        }}
      />
    {/if}
    <div class="min-h-0 flex-1">
      <CourseScreen
        course={store}
        unit={store.openUnit}
        autofocusUnit={made === store.openUnit?.unitId}
        {creating}
        {notice}
        takeFocus={joined}
        onDismissNotice={() => {
          notice = undefined;
        }}
        onCreateFirstUnit={() => void createUnit(store, {})}
        onEditCourse={() => {
          settingsOpen = true;
        }}
        onDeleteCourse={() => {
          deleteCourseOpen = true;
        }}
        onOpenTrash={() => {
          trash = unitTrash(store);
          trashOpen = true;
          void trash.read();
        }}
        onShare={() => {
          sharing = true;
          void store.refreshProgressSummary();
        }}
        showBar={outline.mode === 'button' || (reading && outline.where === 'button')}
        stickyBar={outline.mode === 'button'}
        scrollAttach={outline.follow}
        barAttach={outline.bar}
      >
        {#snippet progress()}
          <CourseProgressBar course={store} />
        {/snippet}
        {#snippet tree({ closeDrawer }: { closeDrawer: (restoreFocus?: boolean) => void })}
          <PageTree
            wiki={store}
            currentId={store.openUnit?.unitId}
            open={treeOpen}
            pageHref={(id: string) => router.href({ ...router.route, unitId: id })}
            onOpenPage={(id: string) => {
              closeDrawer();
              made = undefined;
              router.openUnit(id);
            }}
            onCreate={(draft: PageDraft) => {
              void createUnit(store, draft, () => {
                closeDrawer(false);
              });
            }}
            onRename={(chapter: CourseUnit) => {
              chapterDialog = { mode: 'rename', chapter };
              chapterOpen = true;
            }}
            onDelete={(item: CourseUnit) => {
              deleting = item;
              deleteOpen = true;
            }}
            onMove={(id: string, to: PagePlace) => moveUnit(store, id, to)}
            onMoveTo={(chosen: CourseUnit) => {
              [moving, movingOpen] = [chosen.id, true];
            }}
            {rules}
            labels={COURSE_TREE_LABELS}
            testId="unit-tree"
            busy={creating}
          >
            {#snippet itemLeading(item: CourseUnit)}
              {#if store.canTrackProgress && item.kind === 'unit'}
                {@const done = store.progress.isCompleted(item.id)}
                <span
                  class={['shrink-0', done ? 'text-primary' : 'text-muted-foreground/40']}
                  data-testid="unit-tree-progress"
                  data-completed={done ? 'true' : 'false'}
                  aria-hidden="true"
                >
                  {#if done}<CircleCheckIcon class="size-3.5" />{:else}<CircleIcon
                      class="size-3.5"
                    />{/if}
                </span>
                <span class="sr-only"
                  >{done ? m.course_unit_completed() : m.course_unit_incomplete()}</span
                >
              {/if}
            {/snippet}
            {#snippet headerActions()}
              <div class="flex shrink-0 items-center gap-1">
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 px-2 text-xs text-muted-foreground roomy:min-h-touch"
                  data-testid="new-chapter"
                  disabled={creating}
                  onclick={() => {
                    chapterDialog = { mode: 'create' };
                    chapterOpen = true;
                  }}
                >
                  {m.course_new_chapter()}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 px-2 text-xs text-muted-foreground roomy:min-h-touch"
                  data-testid="new-unit"
                  disabled={creating}
                  onclick={() => {
                    void createUnit(store, {}, () => {
                      closeDrawer(false);
                    });
                  }}
                >
                  {m.course_new_unit()}
                </Button>
              </div>
            {/snippet}
          </PageTree>
        {/snippet}
        {#snippet outlineButton()}
          {#if outline.mode === 'button'}
            <OutlineButton {outline} id="{uid}-outline" />
          {/if}
        {/snippet}
        {#snippet outlineWrap(content: Snippet)}
          <OutlineColumns {outline} id="{uid}-outline">
            {@render content()}
          </OutlineColumns>
        {/snippet}
        {#snippet viewers(id: string)}
          <PageViewers wiki={store} pageId={id} of="unit" large />
        {/snippet}
        {#snippet unitActions(unit: UnitStore)}
          {@const shown = store.unit(unit.unitId)}
          {#if shown?.kind === 'unit'}
            <UnitMenu
              unit={shown}
              onVersions={() => {
                versions = {
                  unitId: unit.unitId,
                  reading: new VersionsReading(unitVersions(unit)),
                };
                versionsOpen = true;
              }}
            />
          {/if}
        {/snippet}
        {#snippet missingUnit()}
          <NotFound {router} />
        {/snippet}
        {#snippet body(unit: UnitStore)}
          <CourseUnitContent {workspace} {store} {unit} outlined={outline.source} />
        {/snippet}
      </CourseScreen>
    </div>
  </div>

  {#if versions && versions.unitId === unitId}
    <BodyVersions bind:open={versionsOpen} reading={versions.reading} container={store} of="unit" />
  {/if}

  <CourseDialogs
    {workspace}
    {store}
    {router}
    {rules}
    {moving}
    bind:movingOpen
    onMoveUnit={(id: string, to: PagePlace) => void moveUnit(store, id, to)}
    {chapterDialog}
    bind:chapterOpen
    onSubmitChapter={(title: string) => void submitChapter(store, title)}
    {deleting}
    bind:deleteOpen
    {trash}
    bind:trashOpen
    bind:sharing
    bind:settingsOpen
    bind:deleteCourseOpen
    onDeleteCourse={() => void deleteCourse(store)}
  />
{/if}
