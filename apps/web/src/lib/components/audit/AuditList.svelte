<!--
  Records of an audit log, the newest first: who did what, to what and whom,
  and when. A person is named "display name (@login)"; one whose account is
  gone as such. A board the reader does not see is "a board", not its title.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { AuditPerson, AuditRecord } from '$lib/logic/audit.ts';

  interface Props {
    records: readonly AuditRecord[];
    locale: string;
    /** Omitted in the app: the reader's zone. A fixed zone is useful for examples. */
    timeZone?: string;
    testId?: string;
  }

  let { records, locale, timeZone, testId = 'audit-list' }: Props = $props();

  function named(person: AuditPerson | null): string {
    if (!person || person.username === '') return m.audit_gone_user();
    return person.displayName
      ? `${person.displayName} (@${person.username})`
      : `@${person.username}`;
  }

  function said(record: AuditRecord): string {
    const actor =
      record.actorKind === 'server_operator' ? m.audit_server_operator() : named(record.actor);
    const subject = named(record.subject);
    const board = record.targetName ?? m.audit_a_board();
    const workspace = record.targetName ?? '';
    switch (record.action) {
      case 'self_added_to_board':
        return m.audit_self_added_to_board({ actor, board });
      case 'added_to_board':
        return m.audit_added_to_board({ actor, subject, board });
      case 'org_admin_joined':
        return m.audit_org_admin_joined({ actor });
      case 'org_admin_left':
        return m.audit_org_admin_left({ actor });
      case 'org_admin_revoked':
        return m.audit_org_admin_revoked({ actor, subject });
      case 'org_admin_granted':
        return m.audit_org_admin_granted({ actor, subject });
      case 'org_admin_removed':
        return m.audit_org_admin_removed({ actor, subject });
      case 'workspace_created':
        return m.audit_workspace_created({ actor, workspace });
      case 'workspace_renamed':
        return m.audit_workspace_renamed({ actor, workspace });
      case 'workspace_deleted':
        return m.audit_workspace_deleted({ actor, workspace });
      case 'workspace_restored':
        return m.audit_workspace_restored({ actor, workspace });
      case 'public_links_disabled':
        return m.audit_public_links_disabled({ actor });
      case 'oidc_provider_configured':
        return m.audit_oidc_configured({ actor });
      case 'oidc_provider_removed':
        return m.audit_oidc_removed({ actor });
      case 'sso_required':
        return m.audit_sso_required({ actor });
      case 'sso_released':
        return m.audit_sso_released({ actor });
      case 'public_links_enabled':
        return m.audit_public_links_enabled({ actor });
    }
  }

  const when = $derived(
    new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short', timeZone }),
  );
</script>

{#if records.length === 0}
  <p class="text-sm text-muted-foreground" data-testid={testId}>{m.audit_none()}</p>
{:else}
  <ol class="flex flex-col gap-2" data-testid={testId}>
    {#each records as record (record.id)}
      <li class="flex flex-col gap-0.5 rounded-md border px-3 py-2" data-action={record.action}>
        <span class="text-sm">{said(record)}</span>
        <time
          class="text-xs text-muted-foreground"
          datetime={new Date(record.createAt).toISOString()}>{when.format(record.createAt)}</time
        >
      </li>
    {/each}
  </ol>
{/if}
