<!--
  Inviting a guest to the board or the wiki that is shared: a link for one
  person, who will see it and nothing else of the workspace. Made here, shown once.
  Where the workspace invites no guests the server refuses, and the section says so.
  Below, the guest links of it that are out (all of them for an
  administrator of the workspace, one's own otherwise), the ones mailed to
  somebody by their address, each to be revoked. They are read when the section
  is made and after each change; a list that cannot be read is left out.
-->
<script lang="ts">
  import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { displayDate } from '$lib/logic/format.ts';
  import type { ContainerKind } from '$lib/logic/wire.ts';
  import {
    isBackendError,
    type GuestContainerRole,
    type Invitation,
    type PeopleAccess,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import InviteLink from './InviteLink.svelte';
  import { shareWords } from './share-words.ts';

  interface Props {
    workspace: Workspace;
    access: PeopleAccess;
    containerId: string;
    kind?: ContainerKind | undefined;
  }

  let { workspace, access, containerId, kind }: Props = $props();
  const words = $derived(shareWords(kind));

  const workspaceName = $derived(nameOfWorkspace(workspace.info, m.shell_workspace_unnamed));

  const uid = $props.id();
  const ROLES: readonly GuestContainerRole[] = ['editor', 'commenter', 'viewer'];
  const ROLE_LABELS: Record<GuestContainerRole, () => string> = {
    editor: () => m['BoardMember.schemeEditor'](),
    commenter: () => m['BoardMember.schemeCommenter'](),
    viewer: () => m['BoardMember.schemeViewer'](),
  };

  let role = $state<GuestContainerRole>('editor');
  let made = $state.raw<{ address: string; token: string } | undefined>(undefined);
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  let links = $state.raw<Invitation[]>([]);
  const containerLinks = $derived(
    links.filter((link) => link.role === 'guest' && link.containerId === containerId),
  );
  const context = $derived({
    locale: workspace.locale,
    timeZone: workspace.regional.timeZone,
    hourCycle: workspace.regional.hourCycle,
  });

  /** Counts the readings: only the latest one's answer is shown. */
  let reading = 0;

  async function readLinks(): Promise<void> {
    const workspaceId = workspace.info?.id;
    if (!workspaceId) return;
    const mine = ++reading;
    let read: Invitation[];
    try {
      read = await access.people.invitations(workspaceId);
    } catch {
      // Making a link does not need the list: without it, the section is as before.
      read = [];
    }
    if (mine === reading) links = read;
  }

  void readLinks();

  async function revoke(link: Invitation): Promise<void> {
    const workspaceId = workspace.info?.id;
    if (working || !workspaceId) return;
    working = true;
    problem = undefined;
    try {
      await access.people.revokeInvitation(workspaceId, link.id);
    } catch (error) {
      problem =
        isBackendError(error) && error.status === 403
          ? m.people_not_allowed({ workspace: workspaceName })
          : m.people_failed();
    } finally {
      await readLinks();
      working = false;
    }
  }

  async function makeLink(): Promise<void> {
    const workspaceId = workspace.info?.id;
    if (working || !workspaceId) return;
    working = true;
    problem = undefined;
    made = undefined;
    try {
      const link = await access.people.inviteGuest(workspaceId, containerId, role);
      made =
        link.token === undefined
          ? undefined
          : { address: access.inviteLink(link.token), token: link.token };
    } catch (error) {
      problem =
        isBackendError(error) && error.status === 403
          ? m.share_guest_off({ workspace: workspaceName })
          : m.people_failed();
    } finally {
      await readLinks();
      working = false;
    }
  }
</script>

<section class="grid gap-2" aria-labelledby="{uid}-guest" data-testid="share-guest">
  <h3 id="{uid}-guest" class="font-medium">{m.share_guest_title()}</h3>
  <p class="text-sm text-muted-foreground">{words.guestHint({ workspace: workspaceName })}</p>
  <div class="flex flex-wrap items-center gap-2">
    <select
      class="h-8 rounded-md border border-input-border bg-background px-2 roomy:h-touch"
      aria-label={m.share_guest_role()}
      bind:value={role}
      disabled={working}
    >
      {#each ROLES as one (one)}
        <option value={one}>{ROLE_LABELS[one]()}</option>
      {/each}
    </select>
    <Button
      variant="outline"
      class="roomy:min-h-touch"
      disabled={working}
      onclick={() => void makeLink()}
      data-testid="share-guest-make"
    >
      {m.share_guest_make()}
    </Button>
  </div>
  {#if problem}
    <p class="text-sm text-destructive" role="alert">{problem}</p>
  {/if}
  {#if made}
    <InviteLink
      link={made.address}
      label={m.people_invite_link()}
      note={m.share_guest_note()}
      testId="share-guest-link"
      delivery={access.people.mail && workspace.info?.id
        ? {
            mail: access.people.mail,
            workspaceId: workspace.info.id,
            token: made.token,
            language: workspace.locale,
            hint: words.guestAddresses(),
          }
        : undefined}
    />
  {/if}
  {#if containerLinks.length > 0}
    <h4 id="{uid}-links" class="text-sm font-medium">{words.guestLinks()}</h4>
    <ul class="grid gap-1" aria-labelledby="{uid}-links" data-testid="share-guest-links">
      {#each containerLinks as link (link.id)}
        <li class="flex min-h-8 items-center gap-2 roomy:min-h-touch">
          <span class="min-w-0 flex-1 text-sm break-words text-muted-foreground">
            {link.recipient
              ? m.share_guest_link_to({
                  recipient: link.recipient,
                  ends: displayDate(link.expiresAt, context),
                })
              : m.share_guest_link_open({ ends: displayDate(link.expiresAt, context) })}
          </span>
          <Button
            variant="ghost"
            size="sm"
            class="roomy:min-h-touch"
            disabled={working}
            onclick={() => void revoke(link)}
          >
            {m.people_link_revoke()}
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
