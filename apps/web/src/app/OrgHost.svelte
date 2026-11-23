<!--
  Organization administration route (`/org/<orgId>`): wires `OrgAdmin`,
  `OrgPage`, and the `SsoRequired` fallback when the session is closed to it.
-->
<script lang="ts">
  import SsoRequired from '$lib/features/account/SsoRequired.svelte';
  import OrgPage from '$lib/features/organization/OrgPage.svelte';
  import type { Workspace, WorkspaceInfo } from '$lib/state/index.ts';
  import { OrgAdmin } from '$lib/state/org-admin.svelte.ts';
  import type { Organizations } from '$lib/state/organization.ts';

  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  interface Props {
    workspace: Workspace;
    organizations: Organizations | undefined;
    orgId: string;
    router: Router;
    openWorkspaces: readonly (WorkspaceInfo & { href: string })[];
    onSso: (orgId: string) => Promise<void>;
    onOpenLinked: () => void;
  }

  let { workspace, organizations, orgId, router, openWorkspaces, onSso, onOpenLinked }: Props =
    $props();

  const orgAdmin = $derived(
    organizations ? new OrgAdmin(organizations, orgId, workspace) : undefined,
  );

  $effect(() => {
    if (workspace.status === 'ready') void orgAdmin?.load();
  });
</script>

{#if orgAdmin}
  {@const closedOrgId = orgAdmin.orgId}
  <OrgPage
    admin={orgAdmin}
    onSsoSignIn={() => onSso(closedOrgId)}
    locale={workspace.locale}
    meId={workspace.me?.id}
    homeHref={router.href({})}
    workspaceHref={(workspaceId: string) => router.href({ workspaceId })}
  >
    {#snippet closed()}
      <SsoRequired
        name=""
        others={openWorkspaces}
        onContinue={() => onSso(closedOrgId)}
        onOpenLinked={() => {
          router.go({});
          onOpenLinked();
        }}
        onGo={(workspaceId: string) => {
          router.go({ workspaceId });
        }}
      />
    {/snippet}
  </OrgPage>
{:else}
  <NotFound {router} />
{/if}
