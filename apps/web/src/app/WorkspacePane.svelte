<!--
  What the pane of the workspace shows, by the address: the board or the wiki
  it names, the way into a workspace closed to this session, a sign-in that
  came back refused, a workspace or a place that is not there, and the
  workspace while it is entered.

  The board or the wiki shown is the one the address names, else the place
  the reader was last on in this workspace if the sidebar still has it, else
  the first of the sidebar (`last-workspace.ts`), its address put in place of
  the workspace's.
-->
<script lang="ts">
  import SignInNotice from '$lib/features/account/SignInNotice.svelte';
  import SsoRequired from '$lib/features/account/SsoRequired.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Workspace, WorkspaceInfo } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import BoardPage from './BoardPage.svelte';
  import { lastPlace, rememberPlace } from './last-workspace.ts';
  import NotFound from './NotFound.svelte';
  import { placeRoute, type Router } from './router.svelte.ts';
  import WikiPage from './WikiPage.svelte';

  interface Props {
    workspace: Workspace;
    router: Router;
    /** A sign-in through a provider that came back refused, to be told. */
    refused: string | null;
    onGoOn: () => void;
    onOpenLinked: () => void;
    /** The reader's workspaces this session opens, with their addresses. */
    openWorkspaces: readonly (WorkspaceInfo & { href: string })[];
    onSso: (orgId: string) => Promise<void>;
  }

  let { workspace, router, refused, onGoOn, onOpenLinked, openWorkspaces, onSso }: Props = $props();

  /** The workspace the address names, entered and read. */
  const shown = $derived(
    workspace.entered === 'ready' && workspace.info?.id === router.route.workspaceId
      ? workspace.info
      : undefined,
  );
  const here = $derived(workspace.asked === router.route.workspaceId);
  /** The board or the wiki of the address. */
  const placeId = $derived(router.route.boardId ?? router.route.wikiId);

  // Without a board or a wiki in the address: the last one the reader was on, or the first.
  $effect(() => {
    if (!shown || placeId) return;
    const listed = workspace.sidebar.flatMap((entry) => entry.boards);
    const last = workspace.me && lastPlace(workspace.me.id, shown.id);
    const chosen = listed.find((board) => board.id === last) ?? listed[0];
    if (chosen) router.go(placeRoute(chosen), { replace: true });
  });

  // The board or the wiki shown is the one to come back to in this workspace.
  $effect(() => {
    const listed = workspace.boards.find((board) => board.id === placeId);
    if (shown && listed && workspace.me) rememberPlace(workspace.me.id, shown.id, listed.id);
  });
</script>

{#if refused}
  <SignInNotice problem={refused} {onGoOn} {onOpenLinked} />
{:else if router.route.unknown || (here && workspace.entered === 'missing')}
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
{:else if !router.route.workspaceId && workspace.workspaces.length === 0}
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
          if (router.route.workspaceId) void workspace.enter(router.route.workspaceId);
        }}>{m.app_retry()}</Button
      >
    </div>
  </div>
{:else if !shown}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else if router.route.boardId}
  <BoardPage {workspace} {router} />
{:else if router.route.wikiId}
  <WikiPage {workspace} {router} />
{:else}
  <div class="grid h-full place-items-center p-6 text-center">
    <p class="text-muted-foreground">{m.app_no_boards()}</p>
  </div>
{/if}
