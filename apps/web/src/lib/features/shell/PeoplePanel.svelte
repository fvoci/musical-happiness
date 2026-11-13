<!--
  The people of the workspace, for who administers it: a link that lets members
  in (made here, shown once), the links that are out, for members and for the
  guests of boards, each to be revoked; whether the admins of boards may
  invite guests; and everybody who is in the workspace, with what they are in it
  and a way to take them out. What `PeopleDialog.svelte` shows while it is open.

  Everything is read when the panel is made, which is each time the dialog
  opens, and again after every request: what the lists showed may be what was
  refused. A refusal says so on one line.
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
  import TeamsPanel from './TeamsPanel.svelte';

  interface Props {
    workspace: Workspace;
    people: People;
    /** The whole address of the page of an invitation, by its token. */
    inviteLink: (token: string) => string;
  }

  let { workspace, people, inviteLink }: Props = $props();

  const uid = $props.id();
  const workspaceName = $derived(workspace.info?.title || m.shell_workspace_unnamed());
  const workspaceId = $derived(workspace.info?.id);

  let members = $state.raw<WorkspaceMember[]>([]);
  let links = $state.raw<Invitation[]>([]);
  let guestsInvited = $state(true);
  /** The link that was just made: its address is shown this once. */
  let made = $state.raw<{ id: string; address: string; token: string } | undefined>(undefined);
  /** `undefined` while the lists are read; then whether they were, or why not. */
  let failure = $state<string | undefined | false>(undefined);
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  /**
   * Counts the readings. The controls that keep what the reader chose (a
   * select, a switch) are made anew with each: one that was refused shows
   * what is so again, not what was asked.
   */
  let version = $state(0);
  /** What is asked before it is done: taking somebody out, or making them a guest, which a change of role does not undo. */
  let asked = $state.raw<{ kind: 'remove' | 'guest'; member: WorkspaceMember } | undefined>(
    undefined,
  );
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

  const memberLinks = $derived(links.filter((link) => link.role === 'member'));
  const guestLinks = $derived(links.filter((link) => link.role === 'guest'));

  const boardTitle = (boardId: string | undefined): string =>
    workspace.boards.find((board) => board.id === boardId)?.title || m.invite_board_untitled();

  function problemOf(error: unknown, otherwise: string): string {
    if (isBackendError(error) && error.reason === 'last_admin')
      return m.people_last_admin({ workspace: workspaceName });
    if (isBackendError(error) && error.status === 403)
      return m.people_not_allowed({ workspace: workspaceName });
    return otherwise;
  }

  async function read(): Promise<void> {
    if (!workspaceId) return;
    try {
      [members, links, guestsInvited] = await Promise.all([
        people.members(workspaceId),
        people.invitations(workspaceId),
        people.guestInvitations(workspaceId),
      ]);
      failure = false;
    } catch (error) {
      failure = problemOf(error, m.people_load_failed({ workspace: workspaceName }));
    }
    version += 1;
  }

  void read();

  /** Does one thing, says so if it is refused, and reads what there is afterwards. */
  async function act(step: (workspaceId: string) => Promise<void>): Promise<void> {
    if (working || !workspaceId) return;
    working = true;
    problem = undefined;
    try {
      await step(workspaceId);
    } catch (error) {
      problem = problemOf(error, m.people_failed());
    } finally {
      await read();
      working = false;
    }
  }

  const makeLink = (): Promise<void> =>
    act(async (id) => {
      made = undefined;
      const link = await people.inviteMembers(id);
      if (link.token !== undefined)
        made = { id: link.id, address: inviteLink(link.token), token: link.token };
    });

  const revoke = (link: Invitation): Promise<void> =>
    act(async (id) => {
      await people.revokeInvitation(id, link.id);
      if (made?.id === link.id) made = undefined;
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

  function chooseRole(member: WorkspaceMember, role: WorkspaceRole): void {
    if (role !== 'guest') {
      void setRole(member, role);
      return;
    }
    // Asked first; left alone, the select shows what the member is again.
    asked = { kind: 'guest', member };
    askOpen = true;
    version += 1;
  }
</script>

{#snippet linkRow(link: Invitation, text: string)}
  <li class="flex min-h-8 items-center gap-2 roomy:min-h-touch">
    <span class="min-w-0 flex-1 text-sm text-muted-foreground">{text}</span>
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
{/snippet}

<div class="grid min-h-0 flex-1 content-start gap-5 overflow-y-auto overscroll-contain p-4">
  <!-- There before it says anything, so that what it says is announced. -->
  <p class="sr-only" role="status">{made ? m.people_link_made() : ''}</p>
  {#if failure === undefined}
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  {:else if failure}
    <p role="alert">{failure}</p>
    <Button class="roomy:min-h-touch" onclick={() => void read()}>{m.app_retry()}</Button>
  {:else}
    {#if problem}
      <p class="text-sm text-destructive" role="alert" data-testid="people-problem">
        {problem}
      </p>
    {/if}

    {#if people.teams}
      <TeamsPanel {workspace} {people} teams={people.teams} membersChanged={version} />
    {/if}

    <section class="grid gap-2" aria-labelledby="{uid}-invite">
      <h3 id="{uid}-invite" class="font-medium">{m.people_invite_title()}</h3>
      {#if made}
        {#key made.id}
          <InviteLink
            link={made.address}
            label={m.people_invite_link()}
            note={m.people_invite_note()}
            testId="people-link"
            delivery={people.mail && workspaceId
              ? {
                  mail: people.mail,
                  workspaceId,
                  token: made.token,
                  language: workspace.locale,
                  hint: m.mail_addresses_hint(),
                }
              : undefined}
          />
        {/key}
      {:else}
        <p class="text-sm text-muted-foreground">
          {m.people_invite_hint({ workspace: workspaceName })}
        </p>
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
            {@render linkRow(
              link,
              m.people_link_row({
                made: displayDate(link.createAt, context),
                ends: displayDate(link.expiresAt, context),
                uses: link.uses,
              }),
            )}
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
        {#key version}
          <Switch
            checked={guestsInvited}
            disabled={working}
            data-testid="people-guests"
            onCheckedChange={(checked: boolean) => void inviteGuests(checked)}
          />
        {/key}
      </label>
      {#if guestLinks.length > 0}
        <ul class="grid gap-1" data-testid="people-guest-links">
          {#each guestLinks as link (link.id)}
            {@render linkRow(
              link,
              m.people_guest_link_row({
                board: boardTitle(link.boardId),
                ends: displayDate(link.expiresAt, context),
              }),
            )}
          {/each}
        </ul>
      {/if}
    </section>

    <section class="grid gap-2" aria-labelledby="{uid}-members">
      <h3 id="{uid}-members" class="font-medium">
        {m.people_members_title({ workspace: workspaceName })}
      </h3>
      <ul class="grid gap-1" data-testid="people-members">
        {#each members as member (member.userId)}
          {@const name = userDisplayName(member)}
          {@const self = member.userId === workspace.me?.id}
          <li
            class="flex min-h-8 flex-wrap items-center gap-2 roomy:min-h-touch"
            data-member={member.username}
          >
            <span class="min-w-0 flex-1 truncate" translate="no">{name}</span>
            <!-- Their own role is changed by another admin, as on a board. -->
            {#key version}
              <select
                class="h-8 rounded-md border border-input-border bg-background px-2 roomy:h-touch"
                aria-label={m.people_role_of({ name })}
                value={member.role}
                disabled={working || self || member.role === 'guest'}
                onchange={(event) => {
                  chooseRole(member, event.currentTarget.value as WorkspaceRole);
                }}
              >
                {#each rolesFor(member) as role (role)}
                  <option value={role}>{ROLE_LABELS[role]()}</option>
                {/each}
              </select>
            {/key}
            <Button
              variant="ghost"
              size="sm"
              class="roomy:min-h-touch"
              disabled={working || self}
              aria-label={m.people_remove_of({ name })}
              onclick={() => {
                asked = { kind: 'remove', member };
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
  title={asked?.kind === 'guest'
    ? m.people_guest_title({ name: userDisplayName(asked.member) })
    : m.people_remove_title({
        name: asked ? userDisplayName(asked.member) : '',
        workspace: workspaceName,
      })}
  description={asked?.kind === 'guest'
    ? m.people_guest_description()
    : m.people_remove_description()}
  confirmLabel={asked?.kind === 'guest' ? m.people_guest_confirm() : m.people_remove()}
  destructive
  onConfirm={() => {
    if (asked?.kind === 'guest') void setRole(asked.member, 'guest');
    else if (asked) void remove(asked.member);
  }}
/>
