<!--
  The app: starts the backend, loads the reader's workspace, and puts the
  screens together. Features do not import each other; this folder is where
  they meet (src/lib/README.md).

  The workspace shown is the one the address names. The root address names
  none: the reader is taken to the one they were last in on this browser if
  it is still theirs, else to their first, the address put in place of the
  root's (`Workspace.firstChoice`, `last-workspace.ts`).

  The link of an invitation (`/invite/<token>`) shows the page of the
  invitation, to whoever is signed in and to whoever is not; where it lets
  them in is then loaded afresh.
  `/org/<organization>` is the screen of the organization's administrators
  (`OrgPage`), in place of the workspace; the way to it is in the sidebar of
  whoever administers one. Only the server has organizations.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import FileViewerHost from '$lib/components/file-viewer/FileViewerHost.svelte';
  import { setViewedFileSource } from '$lib/components/file-viewer/viewing.svelte.ts';
  import AccountLeaving from '$lib/features/account/AccountLeaving.svelte';
  import { daysLeft } from '$lib/features/account/account-deletion.ts';
  import InvitePage from '$lib/features/account/InvitePage.svelte';
  import SignInPage from '$lib/features/account/SignInPage.svelte';
  import AppShell, { type PaneTools } from '$lib/features/shell/AppShell.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    BackendError,
    isBackendError,
    isOpen,
    isSignedOut,
    passwordChanged,
    providePeople,
    startOrganization,
    whileHere,
    Workspace,
    type AcceptedInvitation,
    type Account,
    type LinkedIdentity,
    type NewAccount,
    type Registration,
  } from '$lib/state/index.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import type { ContainerPlace } from '$lib/logic/urls.ts';
  import { fileSourceOf } from '$lib/state/file-source.ts';
  import type { Organizations } from '$lib/state/organization.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { acceptedRoute } from './accepted-route.ts';
  import { followAddress } from './address.svelte.ts';
  import { resendLink, startBackend } from './backend.ts';
  import { lastWorkspace } from './last-workspace.ts';
  import {
    browserLocale,
    currentLocale,
    followAccountLanguage,
    locales,
    shownLanguage,
  } from './locale.ts';
  import OrgHost from './OrgHost.svelte';
  import Problems from './Problems.svelte';
  import { keepReturn, resume, takeReturn } from './return-to.ts';
  import { placeRoute, shownContainerId, type Router } from './router.svelte.ts';
  import { signInWays } from './sign-in-organizations.ts';
  import AccountSettings from './AccountSettings.svelte';
  import PasswordNoticeHost from './PasswordNoticeHost.svelte';
  import { Administered } from './administered.svelte.ts';
  import TopBarTools from './TopBarTools.svelte';
  import WorkspacePane from './WorkspacePane.svelte';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);
  let account = $state.raw<Account | undefined>(undefined);
  let organizations = $state.raw<Organizations | undefined>(undefined);
  /** The organization the reader administers, once asked; the sidebar then leads to it. */
  const administered = new Administered(
    () => workspace,
    () => organizations,
  );
  /**
   * The names of the account just made on this page: its first sign-in
   * copies the welcome board. Another account signed in here gets none.
   */
  let registered: { username: string; email: string } | undefined;

  /** What the address says of a sign-in through a provider that just came back. */
  const cameBack = new URLSearchParams(location.search);
  /** A provider was connected to the account just now: the settings say so, until they close. */
  let justLinked = $state(cameBack.has('oidc_linked'));
  let settingsOpen = $state(cameBack.has('oidc_linked'));
  /** The settings were opened for the connected sign-ins: that part comes into view. */
  let linkedAsked = $state(cameBack.has('oidc_linked'));
  /**
   * A sign-in through a provider came back refused (`/login?oidc_error=…`)
   * while the reader is signed in: told in the page, until they go on.
   */
  let refusal = $state(cameBack.get('oidc_error'));
  const refused = $derived(router.route.unknown ? refusal : null);

  function openLinked(): void {
    linkedAsked = true;
    settingsOpen = true;
  }

  /** A session that is over is signing out, whatever was being asked: told, and passed on. */
  async function signedIn<T>(doing: () => Promise<T>): Promise<T> {
    try {
      return await doing();
    } catch (error) {
      if (isSignedOut(error)) workspace?.report('save', error);
      throw error;
    }
  }

  /** Sends the browser to the provider of an organization, to come back to this address. */
  async function toProvider(orgId: string, linkPassword?: string): Promise<void> {
    if (!account) return;
    // Started by this page's own request; the browser then goes to the provider.
    const address = await account.startOidc(orgId, linkPassword);
    keepReturn(location.pathname);
    location.assign(address);
  }

  const unlink = (identity: LinkedIdentity, password: string): Promise<void> =>
    signedIn(() => account?.unlinkIdentity(identity, password) ?? Promise.resolve());

  onMount(() => {
    resume(router, cameBack);
    void startBackend().then(async (started) => {
      const { backend } = started;
      account = started.account;
      organizations = started.organizations;
      const goneUserName = (): string => m.people_deleted_user();
      const made = new Workspace({ backend, locale: currentLocale, lastWorkspace, goneUserName });
      if (started.people) {
        providePeople(made, {
          people: started.people,
          inviteLink: (token: string) => new URL(router.inviteHref(token), location.origin).href,
        });
      }
      // The bodies of cards and pages name their images and file blocks by file; the board or
      // wiki shown has them. A file block that cannot be saved says why, as an attachment does.
      const files = fileSourceOf(
        backend,
        () =>
          router.route.wikiId ??
          router.route.diaryId ??
          router.route.courseId ??
          made.board?.boardId,
        (error) => {
          made.report('load', error);
        },
      );
      setViewedFileSource(files);
      workspace = made;
      // On the handle of the browser tests and the console too, where there is
      // one (the backend in memory): a test undoes through it.
      const dev = (window as unknown as { fvociDev?: object }).fvociDev;
      if (dev) Object.assign(dev, { workspace: made });
      await made.load(router.route.workspaceId, followsLanguage);
    });
    return () => {
      workspace?.close();
    };
  });

  /**
   * The page in the language of the account, once its settings are read:
   * another device may have chosen it. A page that loads again for it says
   * so, and the workspace reads nothing more on this one: what is under way
   * when a page goes is cut off, and Safari tells each such request as an error.
   */
  function followsLanguage(settings: { language: string }): boolean {
    return followAccountLanguage(settings.language);
  }

  /** Reads the workspace again after a failure, and follows its language as the first load does. */
  async function retryLoad(): Promise<void> {
    await workspace?.load(router.route.workspaceId, followsLanguage);
  }

  /**
   * Saves the language to the account and shows the page in it. A refusal
   * leaves the page as it is, and says so (`changeSettings`).
   */
  async function chooseLanguage(setting: string): Promise<void> {
    if (!workspace) return;
    await workspace.changeSettings({ language: setting });
    if (workspace.settings.language === setting) followAccountLanguage(setting);
  }

  /**
   * Signs out, and starts the page again at its root: nothing of the
   * account that was signed in stays in memory. When the server does not
   * end the session (no answer, a refusal), the page stays as it is and
   * says so; a session that was over already (401) counts as signed out.
   */
  async function signOut(): Promise<void> {
    try {
      await account?.signOut();
    } catch (error) {
      if (!isBackendError(error) || error.status !== 401) {
        workspace?.report('save', error);
        return;
      }
    }
    workspace?.close();
    // Where this account was going is not where the next one starts.
    takeReturn();
    router.restart({});
  }

  /**
   * Asks for the account to be deleted: every session of it is over, and the
   * page starts again at the sign-in, which says in how many days it goes.
   */
  async function askDeletion(username: string, password: string): Promise<void> {
    const due = await signedIn(async () => account?.requestDeletion(username, password));
    workspace?.close();
    takeReturn();
    router.restart({}, { query: `account_leaving=${daysLeft(due ?? 0, Date.now())}` });
  }

  /** The token of the invitation the address is the link of, if it is one. */
  const invitation = $derived(router.route.invitation);

  followAddress(
    () => router,
    () => workspace,
  );

  /** The reader's workspaces that this session opens: where else to go from one it does not. */
  const openWorkspaces = $derived(
    (workspace?.workspaces ?? [])
      .filter(isOpen)
      .map((one) => ({ ...one, href: router.href({ workspaceId: one.id }) })),
  );

  /** Starts an organization from the switcher, and goes to its workspace. */
  async function createOrganization(name: string): Promise<void> {
    const [shown, api] = [workspace, organizations];
    if (!shown || !api) return;
    const made = await signedIn(() =>
      startOrganization(shown, (named) => api.createOrganization(named), name),
    );
    administered.started(made.orgId);
    router.go({ workspaceId: made.id });
  }

  const openContainer = (container: ContainerPlace): void => {
    router.openContainer(container);
  };
  const openTime = (): void => {
    router.go({ meTime: true });
  };
</script>

{#snippet accountSettings()}
  {#if account && workspace}
    <AccountSettings
      {workspace}
      {account}
      {router}
      {signedIn}
      onLink={(orgId: string, password: string) => signedIn(() => toProvider(orgId, password))}
      onUnlink={unlink}
      {justLinked}
      {linkedAsked}
      onDelete={askDeletion}
      onLeft={(orgId: string) => {
        administered.left(orgId);
      }}
    />
  {/if}
{/snippet}

<!-- Back by the browser's "back" from a provider the reader left for: nothing is on its way any more. -->
<svelte:window
  onpageshow={(event: PageTransitionEvent) => {
    if (event.persisted) takeReturn();
  }}
/>

{#if !workspace || workspace.status === 'idle' || workspace.status === 'loading'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <p class="mt-2 text-muted-foreground" role="status">{m.app_loading()}</p>
    </div>
  </main>
{:else if workspace.status === 'leaving' && account}
  {@const waiting = account}
  <AccountLeaving
    read={() => waiting.deletion()}
    onCancel={async () => {
      await waiting.cancelDeletion();
      location.reload();
    }}
    onStale={() => void retryLoad()}
    onSignOut={() => void signOut()}
    locale={currentLocale()}
  />
  <Problems {workspace} />
{:else if invitation !== undefined && account}
  {@const signedInAs = workspace.status === 'ready' ? workspace.me : undefined}
  {@const accounts = account}
  <InvitePage
    token={invitation}
    signedInAs={signedInAs && userDisplayName(signedInAs)}
    preview={(token: string) => accounts.previewInvitation(token)}
    accept={(token: string, made?: NewAccount) =>
      accounts.acceptInvitation(token, made, currentLocale())}
    onResend={resendLink}
    onSignIn={(login: string, password: string) => accounts.signIn(login, password)}
    onSso={(orgId: string) => toProvider(orgId)}
    onJoined={(accepted: AcceptedInvitation) => {
      // Loaded afresh where the invitation let the reader in: what the page
      // has in memory is of whoever was, or was not, signed in before. In
      // place of the link, whose token is not to stay in the history.
      router.restart(acceptedRoute(accepted), { replace: true });
    }}
    onSessionEnded={() => {
      location.reload();
    }}
    homeHref={router.href({})}
  />
{:else if workspace.status === 'signedOut' && account}
  <SignInPage
    leavingDays={Number(cameBack.get('account_leaving')) || undefined}
    onSignIn={async (login: string, password: string) => {
      await account?.signIn(login, password);
      // A session that ended while working here: what is on the page (the
      // open board, undo, problems) was the old session's. Start afresh, as
      // signing out does: at the same address when it is the same person,
      // else at the root, where they are taken to a workspace of theirs.
      const before = workspace?.me;
      if (workspace && before) {
        const now = await workspace.backend.me().catch(() => undefined);
        if (now?.id === before.id) location.reload();
        else router.restart({});
        return;
      }
      // Signed in on no page of the app (`/login` after a refused provider sign-in): the first.
      if (router.route.unknown) router.go({}, { replace: true });
      // A new account is in the language of the page it was made on, and is shown its board first.
      const isNew = login === registered?.username || login === registered?.email;
      await workspace?.load(router.route.workspaceId, isNew ? undefined : followsLanguage);
      // Signed in, yet the next request is not: the browser did not keep the
      // session (a cookie for https on a page over http, or cookies blocked).
      if (workspace?.status === 'signedOut') {
        throw new BackendError(401, {
          reason: 'session_not_kept',
          detail: 'the session was not kept',
        });
      }
      // A new account starts on its own copy of the welcome board, made once.
      if (registered && workspace?.status === 'ready') {
        registered = undefined;
        if (isNew) {
          const signedIn = workspace;
          const where = signedIn.info?.id;
          const welcome = await whileHere(signedIn, () => signedIn.onboard());
          // The page may be on it already (the first board is shown by itself).
          if (welcome && where && router.route.boardId !== welcome) {
            router.openContainer({ id: welcome, workspaceId: where });
          }
        }
      }
      // Last: the page may load again in the account's language.
      if (workspace?.status === 'ready') followsLanguage(workspace.settings);
    }}
    onRegister={async (registration: Registration) => {
      const made = (await account?.register(registration, currentLocale())) ?? 'made';
      if (made === 'made')
        registered = { username: registration.username, email: registration.email };
      return made;
    }}
    onResend={resendLink}
    forgotHref={`${router.href({}).replace(/\/$/, '')}/forgot-password`}
    {...signInWays(account, (orgId: string) => toProvider(orgId), router.href({}))}
  />
{:else if workspace.status !== 'ready'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-lg font-medium" role="alert">{m.app_load_failed()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={() => void retryLoad()}>
        {m.app_retry()}
      </Button>
    </div>
  </main>
{:else if router.route.orgId}
  <OrgHost
    {workspace}
    {organizations}
    orgId={router.route.orgId}
    {router}
    {openWorkspaces}
    onSso={toProvider}
    onOpenLinked={openLinked}
  />
{:else}
  <AppShell
    {workspace}
    orgHref={administered.orgId ? router.href({ orgId: administered.orgId }) : undefined}
    onSwitchWorkspace={(workspaceId: string) => {
      router.go({ workspaceId });
    }}
    onCreateOrganization={organizations ? createOrganization : undefined}
    readRecords={organizations
      ? (workspaceId: string) => organizations?.workspaceRecords(workspaceId) ?? Promise.resolve([])
      : undefined}
    containerId={shownContainerId(router.route)}
    meTime={router.route.meTime === true}
    timeHref={workspace.backend.time ? router.href({ meTime: true }) : undefined}
    onOpenTime={openTime}
    containerHref={(container: ContainerPlace) => router.href(placeRoute(container))}
    sharedHref={(containerId: string, readToken: string) =>
      router.href({ boardId: containerId, readToken })}
    onOpenContainer={openContainer}
    {locales}
    language={shownLanguage(workspace.settings.language)}
    autoLocale={browserLocale()}
    onChooseLanguage={(setting: string) => void chooseLanguage(setting)}
    onSignOut={account ? () => void signOut() : undefined}
    onChangePassword={account
      ? (oldPassword: string, newPassword: string) =>
          signedIn(async () => {
            await account?.changePassword(oldPassword, newPassword);
            if (workspace) await passwordChanged(workspace, newPassword);
          })
      : undefined}
    accountSettings={account ? accountSettings : undefined}
    bind:settingsOpen={
      () => settingsOpen,
      (open: boolean) => {
        settingsOpen = open;
        // Said once, and asked for once: the next opening is the reader's own.
        if (!open) [justLinked, linkedAsked] = [false, false];
      }
    }
  >
    {#snippet search(shown: Workspace)}
      <TopBarTools
        workspace={shown}
        {router}
        {openWorkspaces}
        onOpenLinked={openLinked}
        onSso={(id: string) => toProvider(id)}
      />
    {/snippet}
    {#snippet children({ workspace: shown, addBoard }: PaneTools)}
      <WorkspacePane
        workspace={shown}
        {router}
        {refused}
        onAddBoard={addBoard}
        onGoOn={() => {
          refusal = null;
          router.go({}, { replace: true });
        }}
        onOpenLinked={openLinked}
        {openWorkspaces}
        onSso={(orgId: string) => toProvider(orgId)}
      />
      <!-- After the pane: drawn at its foot, it comes after it when tabbing too. -->
      {#if account}
        <PasswordNoticeHost workspace={shown} {account} onProvider={toProvider} />
      {/if}
    {/snippet}
  </AppShell>
  <Problems {workspace} />
  <FileViewerHost />
{/if}
