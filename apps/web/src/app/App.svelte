<!--
  The app: starts the backend, loads the reader's workspace, and puts the
  screens together. Features do not import each other; this folder is where
  they meet (src/lib/README.md).
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import SignInPage from '$lib/features/account/SignInPage.svelte';
  import AppShell from '$lib/features/shell/AppShell.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    BackendError,
    isBackendError,
    Workspace,
    type Account,
    type Registration,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { startBackend } from './backend.ts';
  import BoardPage from './BoardPage.svelte';
  import {
    browserLocale,
    currentLocale,
    followAccountLanguage,
    localeOfSetting,
    locales,
  } from './locale.ts';
  import Problems from './Problems.svelte';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();

  let workspace = $state.raw<Workspace | undefined>(undefined);
  let account = $state.raw<Account | undefined>(undefined);
  /**
   * The names of the account just made on this page: its first sign-in
   * copies the welcome board. Another account signed in here gets none.
   */
  let registered: { username: string; email: string } | undefined;

  onMount(() => {
    void startBackend().then(async (started) => {
      const { backend } = started;
      account = started.account;
      const made = new Workspace({ backend, locale: currentLocale });
      // The bodies of cards name their images by file; the board that is open says where they are.
      setFileUrlResolver((fileId) =>
        made.board ? backend.fileUrl(made.board.boardId, fileId) : undefined,
      );
      workspace = made;
      // On the handle of the browser tests and the console too, where there is
      // one (the backend in memory): a test undoes through it.
      const dev = (window as unknown as { fvociDev?: object }).fvociDev;
      if (dev) Object.assign(dev, { workspace: made });
      await made.load();
      followLanguage(made);
    });
    return () => {
      workspace?.close();
    };
  });

  /**
   * The page in the language of the account, once its settings are read:
   * another device may have chosen it. A page that loads again for it says
   * so; nothing else is to be done on this one.
   */
  function followLanguage(loaded: Workspace): boolean {
    return loaded.status === 'ready' && followAccountLanguage(loaded.settings.language);
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
    window.location.assign(router.href({}));
  }

  // Without a board in the address, show the first one of the sidebar.
  $effect(() => {
    if (workspace?.status !== 'ready' || router.route.unknown || router.route.boardId) return;
    const first = workspace.sidebar.flatMap((entry) => entry.boards)[0];
    if (first) router.go({ boardId: first.id }, { replace: true });
  });
</script>

{#if !workspace || workspace.status === 'idle' || workspace.status === 'loading'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <p class="mt-2 text-muted-foreground" role="status">{m.app_loading()}</p>
    </div>
  </main>
{:else if workspace.status === 'signedOut' && account}
  <SignInPage
    onSignIn={async (login: string, password: string) => {
      await account?.signIn(login, password);
      // A session that ended while working here: what is on the page (the
      // open board, undo, problems) was the old session's. Start afresh, as
      // signing out does.
      if (workspace?.me) {
        location.reload();
        return;
      }
      await workspace?.load();
      // Signed in, yet the next request is not: the browser did not keep the
      // session (a cookie for https on a page over http, or cookies blocked).
      if (workspace?.status === 'signedOut') {
        throw new BackendError(401, {
          reason: 'session_not_kept',
          detail: 'the session was not kept',
        });
      }
      // A new account starts on its own copy of the welcome board, made once.
      const made = registered;
      if (made && workspace?.status === 'ready') {
        registered = undefined;
        if (login === made.username || login === made.email) {
          const welcome = await workspace.onboard();
          // The page may be on it already (the first board is shown by itself).
          if (welcome && router.route.boardId !== welcome) router.openBoard(welcome);
        }
      }
      // Last: the page may load again in the account's language.
      if (workspace) followLanguage(workspace);
    }}
    onRegister={async (registration: Registration) => {
      await account?.register(registration);
      registered = { username: registration.username, email: registration.email };
    }}
  />
{:else if workspace.status === 'failed' || workspace.status === 'signedOut'}
  <main class="grid min-h-dvh place-items-center p-4">
    <div class="text-center">
      <p class="text-lg font-medium" role="alert">{m.app_load_failed()}</p>
      <Button class="mt-4 roomy:min-h-touch" onclick={() => void workspace?.load()}>
        {m.app_retry()}
      </Button>
    </div>
  </main>
{:else}
  <AppShell
    {workspace}
    boardId={router.route.boardId}
    boardHref={(boardId: string) => router.href({ boardId })}
    onOpenBoard={(boardId: string) => {
      router.openBoard(boardId);
    }}
    {locales}
    language={workspace.settings.language === 'auto'
      ? 'auto'
      : localeOfSetting(workspace.settings.language)}
    autoLocale={browserLocale()}
    onChooseLanguage={(setting: string) => void chooseLanguage(setting)}
    onSignOut={account ? () => void signOut() : undefined}
    onChangePassword={account
      ? async (oldPassword: string, newPassword: string) => {
          try {
            await account?.changePassword(oldPassword, newPassword);
          } catch (error) {
            // The session was over: that is signing out, not a wrong password.
            if (isBackendError(error) && error.status === 401) workspace?.report('save', error);
            throw error;
          }
        }
      : undefined}
  >
    {#if router.route.unknown}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground" role="alert">{m.app_page_not_found()}</p>
      </div>
    {:else if router.route.boardId}
      <BoardPage {workspace} {router} />
    {:else}
      <div class="grid h-full place-items-center p-6 text-center">
        <p class="text-muted-foreground">{m.app_no_boards()}</p>
      </div>
    {/if}
  </AppShell>
  <Problems {workspace} />
{/if}
