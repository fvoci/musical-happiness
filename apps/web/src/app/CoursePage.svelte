<!--
  One course (`/w/<workspace>/courses/<course>/<unit>`): its chapters and units
  as a depth-2 tree on the left, and the open unit's title and collaborative
  body in the center. Composes `features/course/`, `features/wiki/` (`PageTree`,
  `MovePageDialog`, `PageViewers`, `OutlineButton`, `OutlineColumns`),
  `features/card/` (`CardBody`), and `features/shell/` (`TrashDialog`).

  Comments, body versions, `[[` unit references, and public sharing UI are t5.
-->
<script lang="ts">
  import { untrack, type Snippet } from 'svelte';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import CardBody from '$lib/features/card/CardBody.svelte';
  import ChapterNameDialog from '$lib/features/course/ChapterNameDialog.svelte';
  import CourseJoin from '$lib/features/course/CourseJoin.svelte';
  import CourseScreen from '$lib/features/course/CourseScreen.svelte';
  import CourseSettingsDialog from '$lib/features/course/CourseSettingsDialog.svelte';
  import TrashDialog from '$lib/features/shell/TrashDialog.svelte';
  import MovePageDialog from '$lib/features/wiki/MovePageDialog.svelte';
  import OutlineButton from '$lib/features/wiki/OutlineButton.svelte';
  import OutlineColumns from '$lib/features/wiki/OutlineColumns.svelte';
  import { PageOutline } from '$lib/features/wiki/page-outline.svelte.ts';
  import PageTree from '$lib/features/wiki/PageTree.svelte';
  import PageViewers from '$lib/features/wiki/PageViewers.svelte';
  import { COURSE_MOVE_LABELS, COURSE_TREE_LABELS } from '$lib/features/wiki/tree-actions.ts';
  import { TreeOpen } from '$lib/features/wiki/tree-open.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { courseTreeRules, type PagePlace } from '$lib/logic/page-moves.ts';
  import { ancestorsOf, firstUnit, withDescendants } from '$lib/logic/page-tree.ts';
  import {
    CourseStore,
    shownCourse,
    unitTrash,
    whileHere,
    type CourseLooks,
    type CourseUnit,
    type PageDraft,
    type Trash,
    type UnitStore,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import ManagedContainer from './ManagedContainer.svelte';
  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

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
      [trash, trashOpen] = [undefined, false];
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
    untrack(() => shown.showUnit(target));
  });

  let deleting = $state.raw<CourseUnit | undefined>(undefined);
  let deleteOpen = $state(false);
  const below = $derived(
    deleting && course ? withDescendants(course.units, deleting.id).size - 1 : 0,
  );
  let moving = $state<string | undefined>(undefined);
  let movingOpen = $state(false);
  let chapterDialog = $state.raw<{ mode: 'create' } | { mode: 'rename'; chapter: CourseUnit }>({
    mode: 'create',
  });
  let chapterOpen = $state(false);
  let settingsOpen = $state(false);
  let deleteCourseOpen = $state(false);
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
        showBar={outline.mode === 'button' || (reading && outline.where === 'button')}
        stickyBar={outline.mode === 'button'}
        scrollAttach={outline.follow}
        barAttach={outline.bar}
      >
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
        {#snippet missingUnit()}
          <NotFound {router} />
        {/snippet}
        {#snippet body(unit: UnitStore)}
          <div {@attach outline.source}>
            <CardBody container={store} card={unit} of="unit" testId="unit-body" />
          </div>
        {/snippet}
      </CourseScreen>
    </div>
  </div>

  <MovePageDialog
    bind:open={movingOpen}
    wiki={store}
    pageId={moving}
    {rules}
    labels={COURSE_MOVE_LABELS}
    onMove={(id: string, to: PagePlace) => void moveUnit(store, id, to)}
  />

  <ChapterNameDialog
    bind:open={chapterOpen}
    mode={chapterDialog.mode}
    initial={chapterDialog.mode === 'rename' ? chapterDialog.chapter.title : ''}
    onSubmit={(title: string) => void submitChapter(store, title)}
  />

  {@const isChapter = deleting?.kind === 'chapter'}
  {@const delTitle =
    deleting?.title || (isChapter ? m.course_chapter_untitled() : m.course_unit_untitled())}
  <ConfirmDialog
    bind:open={deleteOpen}
    title={isChapter ? m.course_delete_chapter_title() : m.course_delete_unit_title()}
    description={isChapter
      ? below > 0
        ? m.course_delete_chapter_below({ title: delTitle, count: below })
        : m.course_delete_chapter_alone({ title: delTitle })
      : m.course_delete_unit_info({ title: delTitle })}
    confirmLabel={m.shared_delete()}
    destructive
    onConfirm={() => {
      if (deleting) void store.deleteUnit(deleting.id);
    }}
    focusAfter={() =>
      document.querySelector<HTMLElement>(
        '[data-testid="unit-tree"] [role="treeitem"][tabindex="0"]',
      )}
  />

  {#if store.canEdit}
    <TrashDialog
      bind:open={trashOpen}
      {trash}
      {workspace}
      title={m.course_trash()}
      description={m.course_trash_description()}
      untitled={m.course_unit_untitled()}
      belowLabel={(count: number) => m.course_trash_below({ count })}
      testId="unit-trash"
    />
  {/if}

  {#if store.course}
    <CourseSettingsDialog
      bind:open={settingsOpen}
      course={store.course}
      onSave={(looks: CourseLooks) => void store.changeCourse(looks)}
    />
  {/if}

  <ConfirmDialog
    bind:open={deleteCourseOpen}
    title={m.shell_delete_course_title()}
    description={m.shell_delete_course_info({
      title: store.course?.title || m.course_untitled(),
    })}
    confirmLabel={m['DeleteBoardDialog.confirm-delete']()}
    destructive
    onConfirm={() => void deleteCourse(store)}
  />
{/if}
