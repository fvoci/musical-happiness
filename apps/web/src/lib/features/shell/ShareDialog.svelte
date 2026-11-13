<!--
  Sharing a board, for its admins: who of the workspace may find and join it and
  with which role, its members and their roles, adding people of the workspace,
  and the public link (read only, for anyone who has it).

  Every change is sent at once through the board store, which shows it and
  takes it back when the server refuses it. The people offered to add are
  the workspace's oldest ten until something is typed, then the ten the server
  finds by name (D2); members are left out. The public link is read when the
  dialog opens. The reader's own role is not changed here: that is another
  admin's to do, so that nobody locks themselves out by a slip.

  A reader who is no admin of the board sees its members alone, with their roles,
  and changes nothing (`readOnly`). Opened at the members (`at`, from the number
  of members at the head of the board), the dialog begins at that section.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import CheckIcon from '@lucide/svelte/icons/check';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import UserPlusIcon from '@lucide/svelte/icons/user-plus';
  import XIcon from '@lucide/svelte/icons/x';

  import Avatar from '$lib/components/avatar/Avatar.svelte';
  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import {
    isBackendError,
    isGuest,
    peopleOf,
    type BoardRole,
    type BoardStore,
    type Sharing,
    type User,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Switch } from '$lib/ui/switch/index.js';

  import GuestInvite from './GuestInvite.svelte';
  import { ROLE_ORDER, roleOfMember } from './share-logic.ts';

  interface Props {
    open: boolean;
    workspace: Workspace;
    board: BoardStore;
    /** The whole address of the board's public link with a read token. */
    linkOf: (readToken: string) => string;
    /**
     * The members alone: for a board the reader manages without reading it, whose
     * openness and public link are not theirs to change or read.
     */
    membersOnly?: boolean;
    /** The members are shown, not changed: for a reader who is no admin of the board. */
    readOnly?: boolean;
    /** The section the dialog begins at, if not the top. */
    at?: 'members';
  }

  let {
    open = $bindable(),
    workspace,
    board,
    linkOf,
    membersOnly = false,
    readOnly = false,
    at,
  }: Props = $props();

  const uid = $props.id();
  const compact = $derived(page.stage === 'compact');

  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );

  const ROLE_LABELS: Record<BoardRole, () => string> = {
    admin: () => m['BoardMember.schemeAdmin'](),
    editor: () => m['BoardMember.schemeEditor'](),
    commenter: () => m['BoardMember.schemeCommenter'](),
    viewer: () => m['BoardMember.schemeViewer'](),
  };
  /** A minimum role is one a member can have below admin. */
  const MINIMUM_ROLES = ROLE_ORDER.filter((role) => role !== 'admin');

  const nameOf = (userId: string): string =>
    workspace.userName(userId) ?? m['centerPanel.unknown-user']();

  /** Inviting a guest by a link, where the backend has that. */
  const access = $derived(peopleOf(workspace));

  const isOpen = $derived(board.board?.type === 'O');
  const minimumRole = $derived<BoardRole>(
    board.board?.minimumRole && board.board.minimumRole !== 'admin'
      ? board.board.minimumRole
      : 'editor',
  );
  /** The role a person is added with: what joining gives on an open board, an editor otherwise. */
  const addedRole = $derived<BoardRole>(isOpen ? minimumRole : 'editor');

  // ------------------------------------------------------- adding people

  let query = $state('');
  let found = $state.raw<User[]>([]);
  /** Readings of `found` begun: the latest is the one that counts. */
  let searches = 0;

  let searchField = $state<HTMLInputElement | null>(null);

  const candidates = $derived(
    (query.trim() === '' ? workspace.workspaceUsers : found).filter(
      (user) => !board.members.some((member) => member.userId === user.id),
    ),
  );

  async function search(text: string): Promise<void> {
    searches += 1;
    const reading = searches;
    if (text.trim() === '') {
      found = [];
      return;
    }
    const answer = await workspace.searchUsers(text.trim());
    if (reading === searches) found = answer;
  }

  // ---------------------------------------------------------- public link

  let sharing = $state.raw<Sharing | undefined>(undefined);
  /** The server does not allow public links (403). */
  let sharingOff = $state(false);
  let sharingBusy = $state(false);
  let copied = $state(false);
  /** Made anew when a change of the link is refused: the switch shows the link as it is again. */
  let switchVersion = $state(0);
  /** Asking before a new link stops the old one. */
  let confirmOpen = $state(false);

  const link = $derived(sharing?.enabled && sharing.token ? linkOf(sharing.token) : '');

  async function readSharing(): Promise<void> {
    sharing = undefined;
    sharingOff = false;
    try {
      sharing = await workspace.backend.sharing(board.boardId);
    } catch (error) {
      workspace.report('load', error);
    }
  }

  async function changeSharing(enabled: boolean, regenerate = false): Promise<void> {
    if (sharingBusy) return;
    sharingBusy = true;
    copied = false;
    try {
      sharing = await workspace.backend.setSharing(board.boardId, { enabled, regenerate });
    } catch (error) {
      if (isBackendError(error) && error.reason === 'sharing_disabled') sharingOff = true;
      else workspace.report('save', error);
      switchVersion += 1;
    } finally {
      sharingBusy = false;
    }
  }

  let membersHeading = $state<HTMLElement | null>(null);

  /** Opened at the members: the focus goes to their heading, which is scrolled to. */
  function focusAt(event: Event): void {
    if (at !== 'members' || !membersHeading) return;
    event.preventDefault();
    membersHeading.focus();
    membersHeading.scrollIntoView({ block: 'start' });
  }

  /** Each time the dialog opens (its content is made): a fresh search, the link read anew. */
  function opened(): void {
    untrack(() => {
      query = '';
      found = [];
      if (!membersOnly) void readSharing();
    });
  }

  /** The row that held the focus goes away: the focus goes to the search. */
  async function addMember(user: User): Promise<void> {
    searchField?.focus();
    await board.addMember(user.id, addedRole);
  }

  async function removeMember(userId: string): Promise<void> {
    searchField?.focus();
    await board.removeMember(userId);
  }

  async function copyLink(): Promise<void> {
    try {
      await navigator.clipboard.writeText(link);
      copied = true;
    } catch {
      // No clipboard (an insecure page, or denied): the field can still be selected.
      copied = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    {@attach opened}
    onOpenAutoFocus={focusAt}
    data-testid="share-dialog"
    showCloseButton={false}
    class={[
      'flex flex-col gap-0 p-0',
      compact
        ? 'top-0 left-0 h-dvh max-w-none translate-x-0 translate-y-0 rounded-none pt-(--safe-top) pr-(--safe-right) pb-(--safe-bottom) pl-(--safe-left) ring-0 sm:max-w-none'
        : 'max-h-[calc(100dvh-2rem-var(--safe-top)-var(--safe-bottom))] w-[min(32rem,calc(100%-2rem))] max-w-none sm:max-w-none',
    ]}
  >
    <Dialog.Header class="shrink-0 flex-row items-center gap-2 border-b py-2 pr-2 pl-4">
      <Dialog.Title class="min-w-0 flex-1 text-base font-semibold">
        {m['ShareBoard.Title']()}
      </Dialog.Title>
      <Dialog.Close>
        {#snippet child({ props }: { props: Record<string, unknown> })}
          <IconButton {...props} label={m.ui_close()}>
            <XIcon />
          </IconButton>
        {/snippet}
      </Dialog.Close>
    </Dialog.Header>

    <div class="grid min-h-0 flex-1 content-start gap-5 overflow-y-auto overscroll-contain p-4">
      <!-- The workspace. -->
      {#if !membersOnly}
        <section class="grid gap-2" aria-labelledby="{uid}-workspace">
          <h3 id="{uid}-workspace" class="font-medium">{m.share_workspace_title()}</h3>
          <label class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
            <span class="min-w-0 flex-1 text-muted-foreground">{m.share_workspace_open()}</span>
            <Switch
              checked={isOpen}
              data-testid="share-open"
              onCheckedChange={(checked: boolean) => void board.setOpen(checked)}
            />
          </label>
          {#if isOpen}
            <label class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
              <span class="min-w-0 flex-1 text-muted-foreground">{m.share_minimum_role()}</span>
              <select
                class="h-8 rounded-md border border-input-border bg-background px-2 roomy:h-touch"
                data-testid="share-minimum-role"
                value={minimumRole}
                onchange={(event) =>
                  void board.setMinimumRole(event.currentTarget.value as BoardRole)}
              >
                {#each MINIMUM_ROLES as role (role)}
                  <option value={role}>{ROLE_LABELS[role]()}</option>
                {/each}
              </select>
            </label>
          {/if}
        </section>
      {/if}

      {#if access && !readOnly && !isGuest(workspace) && !board.board?.isTemplate}
        <GuestInvite {workspace} {access} boardId={board.boardId} />
      {/if}

      <!-- The members. -->
      <section class="grid gap-2" aria-labelledby="{uid}-members">
        <h3
          bind:this={membersHeading}
          id="{uid}-members"
          tabindex="-1"
          class="scroll-mt-4 font-medium outline-none"
          data-testid="share-members-title"
        >
          {m.share_members_title()}
        </h3>
        <ul class="grid gap-1" data-testid="share-members">
          {#each board.orderedMembers as member (member.userId)}
            {@const name = nameOf(member.userId)}
            {@const mine = member.userId === workspace.me?.id}
            {@const role = roleOfMember(member)}
            <li class="flex min-h-8 items-center gap-2 roomy:min-h-touch" data-member={name}>
              <Avatar {name} colorKey={member.userId} locale={workspace.locale} decorative />
              <span class="min-w-0 flex-1 truncate">
                {name}
                {#if mine}<span class="text-muted-foreground">
                    {m['ShareBoard.userPermissionsYouText']()}</span
                  >{/if}
              </span>
              {#if readOnly}
                <span class="shrink-0 text-muted-foreground" data-testid="share-member-role">
                  {role === 'none' ? m.share_role_none() : ROLE_LABELS[role]()}
                </span>
              {:else}
                <!-- The reader's own role is another admin's to change: no locking oneself out. -->
                <select
                  class="h-8 rounded-md border border-input-border bg-background px-2 disabled:opacity-60 roomy:h-touch"
                  aria-label={m.share_role_of({ name })}
                  title={mine ? m.share_you_note() : undefined}
                  disabled={mine}
                  value={role}
                  onchange={(event) =>
                    void board.setMemberRole(member.userId, event.currentTarget.value as BoardRole)}
                >
                  {#if role === 'none'}
                    <option value="none" disabled>{m.share_role_none()}</option>
                  {/if}
                  {#each ROLE_ORDER as role (role)}
                    <option value={role}>{ROLE_LABELS[role]()}</option>
                  {/each}
                </select>
                <IconButton
                  label={m.share_remove({ name })}
                  class="text-muted-foreground"
                  disabled={mine}
                  onclick={() => void removeMember(member.userId)}
                >
                  <XIcon />
                </IconButton>
              {/if}
            </li>
          {/each}
        </ul>

        {#if !readOnly}
          <Input
            bind:ref={searchField}
            type="search"
            bind:value={query}
            oninput={() => void search(query)}
            autocomplete="off"
            aria-label={m['ShareBoard.searchPlaceholder']()}
            placeholder={m['ShareBoard.searchPlaceholder']()}
            data-testid="share-search"
            class="roomy:h-touch roomy:text-base"
          />
          {#if candidates.length > 0}
            <ul class="grid gap-1" data-testid="share-candidates">
              {#each candidates as user (user.id)}
                {@const name = userDisplayName(user)}
                <li class="flex min-h-8 items-center gap-2 roomy:min-h-touch">
                  <Avatar {name} colorKey={user.id} locale={workspace.locale} decorative />
                  <span class="min-w-0 flex-1 truncate">{name}</span>
                  <Button
                    variant="outline"
                    size="sm"
                    class="roomy:min-h-touch"
                    aria-label={m.share_add({ name })}
                    onclick={() => void addMember(user)}
                  >
                    <UserPlusIcon aria-hidden="true" />
                    {m.share_add_short()}
                  </Button>
                </li>
              {/each}
            </ul>
          {/if}
          <!-- In the page from the start: a live region that appears with its words is often not read. -->
          <p class="text-muted-foreground empty:hidden" role="status">
            {candidates.length === 0 && query.trim() !== '' ? m.share_nobody_found() : ''}
          </p>
        {/if}
      </section>

      <!-- The public link. -->
      {#if !membersOnly}
        <section class="grid gap-2 border-t pt-4" aria-labelledby="{uid}-public">
          <h3 id="{uid}-public" class="font-medium">{m['ShareBoard.PublishTitle']()}</h3>
          <label class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
            <span class="min-w-0 flex-1 text-muted-foreground">
              {m['ShareBoard.PublishDescription']()}
            </span>
            {#key switchVersion}
              <Switch
                checked={sharing?.enabled ?? false}
                disabled={!sharing || sharingBusy || sharingOff}
                data-testid="share-public"
                onCheckedChange={(checked: boolean) => void changeSharing(checked)}
              />
            {/key}
          </label>
          {#if sharingOff}
            <p class="text-muted-foreground" role="status">{m.share_public_off()}</p>
          {/if}
          {#if link}
            <div class="flex items-center gap-1">
              <Input
                readonly
                value={link}
                aria-label={m.share_public_link()}
                data-testid="share-link"
                class="min-w-0 flex-1 roomy:h-touch"
                onfocus={(event: FocusEvent) => {
                  (event.currentTarget as HTMLInputElement).select();
                }}
              />
              <IconButton
                label={copied ? m['ShareBoard.copiedLink']() : m['ShareBoard.copyLink']()}
                onclick={() => void copyLink()}
              >
                {#if copied}<CheckIcon />{:else}<CopyIcon />{/if}
              </IconButton>
              <IconButton
                label={m['ShareBoard.regenerate']()}
                disabled={sharingBusy}
                data-testid="share-regenerate"
                onclick={() => {
                  confirmOpen = true;
                }}
              >
                <RefreshCwIcon />
              </IconButton>
            </div>
          {/if}
        </section>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<ConfirmDialog
  bind:open={confirmOpen}
  title={m['ShareBoard.regenerate']()}
  description={m['ShareBoard.confirmRegenerateToken']()}
  confirmLabel={m['ShareBoard.regenerate']()}
  onConfirm={() => void changeSharing(true, true)}
/>
