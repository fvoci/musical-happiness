<!--
  The organization screen (`/org/<organization>`), for its administrators
  only: the workspaces (`OrgWorkspaces.svelte`), the members and their roles,
  the switch of the public links, and what was done to the organization.
  An administrator manages here; what is in a workspace they read by joining
  it. A refused change is told in a line at the top; the lists show what the
  server has.
-->
<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';

  import AuditList from '$lib/components/audit/AuditList.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { OrgAdmin, OrgRefusal } from '$lib/state/org-admin.svelte.ts';
  import type { OrgMember } from '$lib/state/organization.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Switch } from '$lib/ui/switch/index.js';
  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';

  import OrgWorkspaces from './OrgWorkspaces.svelte';

  interface Props {
    admin: OrgAdmin;
    locale: string;
    /** The reader: stepping down from administering the organization is asked first. */
    meId: string | undefined;
    homeHref: string;
    workspaceHref: (workspaceId: string) => string;
  }

  let { admin, locale, meId, homeHref, workspaceHref }: Props = $props();

  const REFUSALS: Record<OrgRefusal, () => string> = {
    name_taken: m.org_refused_name_taken,
    default_workspace: m.org_refused_default_workspace,
    last_org_admin: m.org_refused_last_admin,
    not_found: m.org_refused_not_found,
    failed: m.org_refused_failed,
  };

  let stepDownOpen = $state(false);

  const named = (member: OrgMember): string =>
    member.displayName ? `${member.displayName} (@${member.username})` : `@${member.username}`;
</script>

<main
  class="mx-auto flex min-h-dvh w-full max-w-3xl flex-col gap-6 px-4 py-6"
  data-testid="org-page"
>
  <header class="flex items-center gap-2">
    <a
      href={homeHref}
      class="inline-flex size-9 items-center justify-center rounded-md hover:bg-accent"
      aria-label={m.org_back()}
      data-testid="org-back"
    >
      <ArrowLeftIcon class="size-5" aria-hidden="true" />
    </a>
    <div class="min-w-0">
      <h1 class="truncate text-xl font-semibold tracking-tight">{m.org_title()}</h1>
      {#if admin.organization?.name}
        <p class="truncate text-sm text-muted-foreground">{admin.organization.name}</p>
      {/if}
    </div>
  </header>

  {#if admin.status === 'loading'}
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  {:else if admin.status === 'missing'}
    <!-- Not theirs to administer: never, or no longer (stepped down, or made a member). -->
    <p class="text-muted-foreground" role="alert" data-testid="org-missing">{m.org_missing()}</p>
  {:else if admin.status === 'failed'}
    <div>
      <p class="text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={() => void admin.load()}
        >{m.app_retry()}</Button
      >
    </div>
  {:else}
    {#if admin.refusal}
      <p
        class="rounded-md border border-destructive/50 px-3 py-2 text-sm"
        role="alert"
        data-testid="org-refusal"
      >
        {REFUSALS[admin.refusal]()}
      </p>
    {/if}

    <OrgWorkspaces {admin} {workspaceHref} />

    <section class="flex flex-col gap-3" aria-labelledby="org-members">
      <h2 id="org-members" class="text-base font-semibold">{m.org_members()}</h2>
      <ul class="flex flex-col gap-2" data-testid="org-members">
        {#each admin.members as member (member.userId)}
          <li
            class="flex items-center gap-2 rounded-md border px-3 py-2"
            data-username={member.username}
          >
            <span class="min-w-0 flex-1 truncate">{named(member)}</span>
            <span class="text-xs text-muted-foreground">
              {member.role === 'admin' ? m.org_role_admin() : m.org_role_member()}
            </span>
            <Button
              size="sm"
              variant="outline"
              disabled={admin.busy}
              aria-label={member.role === 'admin'
                ? member.userId === meId
                  ? m.org_step_down()
                  : m.org_make_member_named({ name: named(member) })
                : m.org_make_admin_named({ name: named(member) })}
              onclick={() => {
                if (member.userId === meId) stepDownOpen = true;
                else
                  void admin.setRole(member.userId, member.role === 'admin' ? 'member' : 'admin');
              }}
            >
              {member.role === 'admin'
                ? member.userId === meId
                  ? m.org_step_down()
                  : m.org_make_member()
                : m.org_make_admin()}
            </Button>
          </li>
        {/each}
      </ul>
    </section>

    <section class="flex flex-col gap-3" aria-labelledby="org-settings">
      <h2 id="org-settings" class="text-base font-semibold">{m.org_settings()}</h2>
      <label class="flex items-start gap-3 rounded-md border px-3 py-2">
        <span class="min-w-0 flex-1">
          <span class="block">{m.org_public_links()}</span>
          <span class="block text-xs text-muted-foreground">{m.org_public_links_hint()}</span>
        </span>
        <!-- Drawn anew with what the server has after each change: a refused one flips back. -->
        {#key admin.organization}
          <Switch
            checked={admin.organization?.publicLinks ?? true}
            disabled={admin.busy}
            data-testid="org-public-links"
            onCheckedChange={(on: boolean) => void admin.setPublicLinks(on)}
          />
        {/key}
      </label>
    </section>

    <section class="flex flex-col gap-3" aria-labelledby="org-records">
      <h2 id="org-records" class="text-base font-semibold">{m.org_records()}</h2>
      <AuditList records={admin.records} {locale} testId="org-records" />
    </section>
  {/if}
</main>

<ConfirmDialog
  bind:open={stepDownOpen}
  title={m.org_step_down_title()}
  description={m.org_step_down_info()}
  confirmLabel={m.org_step_down()}
  destructive
  onConfirm={() => {
    if (meId) void admin.setRole(meId, 'member');
  }}
/>
