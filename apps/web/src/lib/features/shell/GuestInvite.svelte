<!--
  Inviting a guest to the board that is shared: a link for one person, who
  will see this board and nothing else of the workspace. Made here, shown once.
  Where the workspace invites no guests the server refuses, and the section says so.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    isBackendError,
    type GuestBoardRole,
    type PeopleAccess,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import InviteLink from './InviteLink.svelte';

  interface Props {
    workspace: Workspace;
    access: PeopleAccess;
    boardId: string;
  }

  let { workspace, access, boardId }: Props = $props();

  const uid = $props.id();
  const ROLES: readonly GuestBoardRole[] = ['editor', 'commenter', 'viewer'];
  const ROLE_LABELS: Record<GuestBoardRole, () => string> = {
    editor: () => m['BoardMember.schemeEditor'](),
    commenter: () => m['BoardMember.schemeCommenter'](),
    viewer: () => m['BoardMember.schemeViewer'](),
  };

  let role = $state<GuestBoardRole>('editor');
  let made = $state.raw<{ address: string; token: string } | undefined>(undefined);
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);

  async function makeLink(): Promise<void> {
    const workspaceId = workspace.info?.id;
    if (working || !workspaceId) return;
    working = true;
    problem = undefined;
    made = undefined;
    try {
      const link = await access.people.inviteGuest(workspaceId, boardId, role);
      made =
        link.token === undefined
          ? undefined
          : { address: access.inviteLink(link.token), token: link.token };
    } catch (error) {
      problem =
        isBackendError(error) && error.status === 403 ? m.share_guest_off() : m.people_failed();
    } finally {
      working = false;
    }
  }
</script>

<section class="grid gap-2" aria-labelledby="{uid}-guest" data-testid="share-guest">
  <h3 id="{uid}-guest" class="font-medium">{m.share_guest_title()}</h3>
  <p class="text-sm text-muted-foreground">{m.share_guest_hint()}</p>
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
          }
        : undefined}
    />
  {/if}
</section>
