<!--
  What the pane of the workspace shows, by the address: the board, wiki,
  course, diary, or personal/learning screen it names, SSO/password lock gates,
  or not-found/loading states.
-->
<script lang="ts">
  import { LazyChunk } from '$lib/chunk-reload.svelte.ts';
  import OwnSpaceLocked from '$lib/features/account/OwnSpaceLocked.svelte';
  import SignInNotice from '$lib/features/account/SignInNotice.svelte';
  import SsoRequired from '$lib/features/account/SsoRequired.svelte';
  import WorkspaceStart from '$lib/features/shell/WorkspaceStart.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    confirmPassword,
    isOwnSpace,
    type Workspace,
    type WorkspaceInfo,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import BoardPage from './BoardPage.svelte';
  import { lastPlace, rememberPlace } from './last-workspace.ts';
  import LazyView from './LazyView.svelte';
  import NotFound from './NotFound.svelte';
  import { placeRoute, shownContainerId, workspaceIdOf, type Router } from './router.svelte.ts';

  const wikiPageChunk = new LazyChunk(() => import('./WikiPage.svelte'));
  const coursePageChunk = new LazyChunk(() => import('./CoursePage.svelte'));
  const diaryPageChunk = new LazyChunk(() => import('./DiaryPage.svelte'));
  const myTimePageChunk = new LazyChunk(() => import('./MyTimePage.svelte'));
  const learningTodayChunk = new LazyChunk(() => import('./LearningTodayPage.svelte'));
  const todayReviewsChunk = new LazyChunk(() => import('./TodayReviewsPage.svelte'));

  interface Props {
    workspace: Workspace;
    router: Router;
    refused: string | null;
    onGoOn: () => void;
    onOpenLinked: () => void;
    openWorkspaces: readonly (WorkspaceInfo & { href: string })[];
    onSso: (orgId: string) => Promise<void>;
    onAddBoard?: (() => void) | undefined;
  }

  let {
    workspace,
    router,
    refused,
    onGoOn,
    onOpenLinked,
    openWorkspaces,
    onSso,
    onAddBoard,
  }: Props = $props();

  const routeWorkspaceId = $derived(workspaceIdOf(router.route));
  const shown = $derived(
    workspace.entered === 'ready' && workspace.info?.id === routeWorkspaceId
      ? workspace.info
      : undefined,
  );
  const here = $derived(workspace.asked === routeWorkspaceId);
  const placeId = $derived(shownContainerId(router.route));

  // Without a container or learning screen in the address: the last place in this workspace, or the first.
  $effect(() => {
    if (!shown || router.route.kind !== 'workspace') return;
    const listed = workspace.sidebar.flatMap((entry) => entry.containers);
    const last = workspace.me && lastPlace(workspace.me.id, shown.id);
    const chosen = listed.find((container) => container.id === last) ?? listed[0];
    if (chosen) router.go(placeRoute(chosen), { replace: true });
  });

  // Remember the active container for this workspace.
  $effect(() => {
    const listed = workspace.containers.find((container) => container.id === placeId);
    if (shown && listed && workspace.me) rememberPlace(workspace.me.id, shown.id, listed.id);
  });
</script>

{#if refused}
  <SignInNotice problem={refused} {onGoOn} {onOpenLinked} />
{:else if router.route.kind === 'meTime'}
  <LazyView chunk={myTimePageChunk}>
    {#snippet children(MyTimePage)}
      <MyTimePage {workspace} {router} />
    {/snippet}
  </LazyView>
{:else if router.route.kind === 'unknown' || (here && workspace.entered === 'missing')}
  <NotFound {router} />
{:else if here && workspace.entered === 'sso' && workspace.closed?.orgId}
  {@const orgId = workspace.closed.orgId}
  {#key orgId}
    <SsoRequired
      name={workspace.closed.title}
      others={openWorkspaces}
      onContinue={() => onSso(orgId)}
      {onOpenLinked}
      onGo={(workspaceId: string) => {
        router.go({ workspaceId });
      }}
    />
  {/key}
{:else if here && workspace.entered === 'password'}
  <OwnSpaceLocked
    username={workspace.me?.username ?? ''}
    onConfirm={(password: string) => confirmPassword(workspace, password)}
  />
{:else if !routeWorkspaceId && workspace.workspaces.length === 0}
  <div class="grid h-full place-items-center p-6 text-center">
    <p class="text-muted-foreground">{m.app_no_boards()}</p>
  </div>
{:else if here && workspace.entered === 'failed'}
  <div class="grid h-full place-items-center p-6 text-center" data-testid="enter-failed">
    <div>
      <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      <Button
        class="mt-4 roomy:min-h-touch"
        onclick={() => {
          if (routeWorkspaceId) void workspace.enter(routeWorkspaceId);
        }}>{m.app_retry()}</Button
      >
    </div>
  </div>
{:else if !shown}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if router.route.kind === 'board'}
  <BoardPage {workspace} {router} />
{:else if router.route.kind === 'wiki'}
  <LazyView chunk={wikiPageChunk}>
    {#snippet children(WikiPage)}
      <WikiPage {workspace} {router} />
    {/snippet}
  </LazyView>
{:else if router.route.kind === 'course'}
  <LazyView chunk={coursePageChunk}>
    {#snippet children(CoursePage)}
      <CoursePage {workspace} {router} />
    {/snippet}
  </LazyView>
{:else if router.route.kind === 'reviewsToday'}
  <LazyView chunk={todayReviewsChunk}>
    {#snippet children(TodayReviewsPage)}
      <TodayReviewsPage {workspace} {router} />
    {/snippet}
  </LazyView>
{:else if router.route.kind === 'learningToday'}
  <LazyView chunk={learningTodayChunk}>
    {#snippet children(LearningTodayPage)}
      <LearningTodayPage {workspace} {router} />
    {/snippet}
  </LazyView>
{:else if router.route.kind === 'diary'}
  {#key router.route.diaryId}
    <LazyView chunk={diaryPageChunk}>
      {#snippet children(DiaryPage)}
        <DiaryPage {workspace} {router} />
      {/snippet}
    </LazyView>
  {/key}
{:else}
  <WorkspaceStart own={isOwnSpace(shown)} {onAddBoard} />
{/if}
