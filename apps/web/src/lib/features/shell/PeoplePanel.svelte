<!--
  The people of the workspace, for who administers it: a link that lets members
  in (made here, shown once, revoked here), whether the admins of boards may
  invite guests, and everybody who is in the workspace, with what they are in it
  and a way to take them out. What `PeopleDialog.svelte` shows while it is open.

  Everything is read when the panel is made, which is each time the dialog
  opens. A request that is refused says so on one line, and the lists are read
  again: what they showed may be what was refused.
-->
<script lang="ts">
  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { displayDate } from '$lib/logic/format.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import {
    isBackendError,
    type Invitation,
    type People,
    type Workspace,
    type WorkspaceMember,
    type WorkspaceRole,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Switch } from '$lib/ui/switch/index.js';

  import InviteLink from './InviteLink.svelte';

  interface Props {
    workspace: Workspace;
    people: People;
    /** The whole address of the page of an invitation, by its token. */
    inviteLink: (token: string) => string;
  }

  let { workspace, people, inviteLink }: Props = $props();

  const uid = $props.id();
  const workspaceId = $derived(workspace.info?.id);

  let members = $state.raw<WorkspaceMember[]>([]);
  let links = $state.raw<Invitation[]>([]);
  let guestsInvited = $state(true);
  /** The link that was just made: its address is shown this once. */
  let made = $state.raw<string | undefined>(undefined);
  let status = $state<'loading' | 'ready' | 'failed'>('loading');
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  let leaving = $state.raw<WorkspaceMember | undefined>(undefined);
  let askOpen = $state(false);

  const ROLE_LABELS: Record<WorkspaceRole, () => string> = {
    admin: m.people_role_admin,
    member: m.people_role_member,
    guest: m.people_role_guest,
  };

  /** What a member may be made by a change of role: what the server takes. */
  function rolesFor(member: WorkspaceMember): WorkspaceRole[] {
    if (member.role === 'guest') return ['guest'];
    return member.role === 'admin' ? ['admin', 'member'] : ['admin', 'member', 'guest'];
  }

  const context = $derived({
    locale: workspace.locale,
    timeZone: workspace.regional.timeZone,
    hourCycle: workspace.regional.hourCycle,
  });

  /** The links that let members in; those for guests are made and seen on their boards. */
  const memberLinks = $derived(links.filter((link) => link.role === 'member'));

  async function read(): Promise<void> {
    if (!workspaceId) return;
    try {
      [members, links, guestsInvited] = await Promise.all([
        people.members(workspaceId),
        people.invitations(workspaceId),
        people.guestInvitations(workspaceId),
      ]);
      status = 'ready';
    } catch {
      status = 'failed';
    }
  }

  void read();

  function problemOf(error: unknown): string {
    if (isBackendError(error) && error.reason === 'last_admin') return m.people_last_admin();
    if (isBackendError(error) && error.status === 403) return m.people_not_allowed();
    return m.people_failed();
  }

  /** Does one thing, says so if it is refused, and reads what there is afterwards. */
  async function act(step: (workspaceId: string) => Promise<void>): Promise<void> {
    if (working || !workspaceId) return;
    working = true;
    problem = undefined;
    try {
      await step(workspaceId);
    } catch (error) {
      problem = problemOf(error);
    } finally {
      await read();
      working = false;
    }
  }

  const makeLink = (): Promise<void> =>
    act(async (id) => {
      const link = await people.inviteMembers(id);
      made = link.token === undefined ? undefined : inviteLink(link.token);
    });

  const revoke = (link: Invitation): Promise<void> =>
    act(async (id) => {
      await people.revokeInvitation(id, link.id);
      made = undefined;
    });

  const setRole = (member: WorkspaceMember, role: WorkspaceRole): Promise<void> =>
    act(async (id) => {
      await people.setRole(id, member.userId, role);
    });

  const remove = (member: WorkspaceMember): Promise<void> =>
    act(async (id) => {
      await people.removeMember(id, member.userId);
    });

  const inviteGuests = (enabled: boolean): Promise<void> =>
    act(async (id) => {
      guestsInvited = await people.setGuestInvitations(id, enabled);
    });
</script>

<div class="grid min-h-0 flex-1 content-start gap-5 overflow-y-auto overscroll-contain p-4">
  {#if status === 'loading'}
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  {:else if status === 'failed'}
    <p role="alert">{m.people_failed()}</p>
    <Button class="roomy:min-h-touch" onclick={() => void read()}>{m.app_retry()}</Button>
  {:else}
    {#if problem}
      <p class="text-sm text-destructive" role="alert" data-testid="people-problem">
        {problem}
      </p>
    {/if}

    <section class="grid gap-2" aria-labelledby="{uid}-invite">
      <h3 id="{uid}-invite" class="font-medium">{m.people_invite_title()}</h3>
      {#if made}
        <InviteLink
          link={made}
          label={m.people_invite_link()}
          note={m.people_invite_note()}
          testId="people-link"
        />
      {:else}
        <p class="text-sm text-muted-foreground">{m.people_invite_hint()}</p>
      {/if}
      <Button
        variant="outline"
        class="justify-self-start roomy:min-h-touch"
        disabled={working}
        onclick={() => void makeLink()}
        data-testid="people-make-link"
      >
        {m.people_invite_make()}
      </Button>
      {#if memberLinks.length > 0}
        <ul class="grid gap-1" data-testid="people-links">
          {#each memberLinks as link (link.id)}
            <li class="flex min-h-8 items-center gap-2 roomy:min-h-touch">
              <span class="min-w-0 flex-1 text-sm text-muted-foreground">
                {m.people_link_row({
                  made: displayDate(link.createAt, context),
                  ends: displayDate(link.expiresAt, context),
                  uses: link.uses,
                })}
              </span>
              <Button
                variant="ghost"
                size="sm"
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

    <section class="grid gap-2" aria-labelledby="{uid}-guests">
      <h3 id="{uid}-guests" class="font-medium">{m.people_guests_title()}</h3>
      <label class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
        <span class="min-w-0 flex-1 text-sm text-muted-foreground">
          {m.people_guests_switch()}
        </span>
        <Switch
          checked={guestsInvited}
          disabled={working}
          data-testid="people-guests"
          onCheckedChange={(checked: boolean) => void inviteGuests(checked)}
        />
      </label>
    </section>

    <section class="grid gap-2" aria-labelledby="{uid}-members">
      <h3 id="{uid}-members" class="font-medium">{m.people_members_title()}</h3>
      <ul class="grid gap-1" data-testid="people-members">
        {#each members as member (member.userId)}
          {@const name = userDisplayName(member)}
          {@const self = member.userId === workspace.me?.id}
          <li class="flex min-h-8 flex-wrap items-center gap-2 roomy:min-h-touch">
            <span class="min-w-0 flex-1 truncate" translate="no">{name}</span>
            <select
              class="h-8 rounded-md border border-input-border bg-background px-2 roomy:h-touch"
              aria-label={m.people_role_of({ name })}
              value={member.role}
              disabled={working || member.role === 'guest'}
              onchange={(event) => void setRole(member, event.currentTarget.value as WorkspaceRole)}
            >
              {#each rolesFor(member) as role (role)}
                <option value={role}>{ROLE_LABELS[role]()}</option>
              {/each}
            </select>
            <Button
              variant="ghost"
              size="sm"
              disabled={working || self}
              aria-label={m.people_remove_of({ name })}
              onclick={() => {
                leaving = member;
                askOpen = true;
              }}
            >
              {m.people_remove()}
            </Button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<ConfirmDialog
  bind:open={askOpen}
  title={m.people_remove_title({ name: leaving ? userDisplayName(leaving) : '' })}
  description={m.people_remove_description()}
  confirmLabel={m.people_remove()}
  destructive
  onConfirm={() => {
    if (leaving) void remove(leaving);
  }}
/>
