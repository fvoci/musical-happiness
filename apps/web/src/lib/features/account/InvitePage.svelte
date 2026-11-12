<!--
  The page of an invitation: what the link of an invitation opens
  (`/invite/<token>`).

  It says who invites and where to, and lets in whoever reads it: somebody
  signed in joins with a press; somebody who is not makes an account, or
  signs in with the one they have, and joins with that. A link that lets
  nobody in is said to be so in one way, whatever is wrong with it: the
  server tells no more.

  Making an account does not sign in on the server, so the page signs in
  right after with what was typed. A refusal is told as `problems.ts` says.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    isBackendError,
    type AcceptedInvitation,
    type InvitationPreview,
    type NewAccount,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  import AccountFields from './AccountFields.svelte';
  import {
    accountFieldsUnder,
    formProblemsOf,
    type AccountField,
    type FieldProblems,
  } from './problems.ts';

  interface Props {
    /** The token of the link, from the address. */
    token: string;
    /** The name of whoever is signed in; undefined for nobody. */
    signedInAs: string | undefined;
    preview: (token: string) => Promise<InvitationPreview>;
    accept: (token: string, account?: NewAccount) => Promise<AcceptedInvitation>;
    onSignIn: (login: string, password: string) => Promise<void>;
    /** Shows where the invitation let the reader in. */
    onJoined: (accepted: AcceptedInvitation) => void;
    /** The address of the app's first page, for a link that lets nobody in. */
    homeHref: string;
  }

  let { token, signedInAs, preview, accept, onSignIn, onJoined, homeHref }: Props = $props();

  /** What the link shows; `undefined` for one that lets nobody in. */
  const shown = $derived(
    preview(token).then(
      (found) => ({ found, failed: false }),
      (error: unknown) => ({
        found: undefined,
        // Anything but "no such link" may pass: the page offers to try again.
        failed: !(isBackendError(error) && error.reason === 'invitation_unavailable'),
      }),
    ),
  );

  let mode = $state<'register' | 'signIn'>('register');
  let login = $state('');
  let username = $state('');
  let displayName = $state('');
  let email = $state('');
  let password = $state('');
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  let fieldProblems = $state.raw<FieldProblems>({});
  /** The link stopped letting anybody in while the page was open. */
  let gone = $state(false);
  /** Signed in on this page with an account they had: joining is one more step, taken at once. */
  let signedIn = $state(false);

  /** The field of each form by the member of its request, a JSON Pointer. */
  const FIELDS: Record<'register' | 'signIn', Record<string, AccountField>> = {
    register: accountFieldsUnder('/account'),
    signIn: { '/username': 'login', '/email': 'login', '/password': 'password' },
  };

  async function run(
    form: 'register' | 'signIn' | undefined,
    step: () => Promise<void>,
  ): Promise<void> {
    if (working) return;
    working = true;
    problem = undefined;
    fieldProblems = {};
    try {
      await step();
    } catch (error) {
      if (isBackendError(error) && error.reason === 'invitation_unavailable') {
        gone = true;
        return;
      }
      const said = formProblemsOf(error, form ? FIELDS[form] : {});
      fieldProblems = said.fields;
      problem = said.problem;
      if (said.first !== undefined) {
        await tick();
        document.getElementById(`account-${said.first}`)?.focus();
      }
    } finally {
      working = false;
    }
  }

  /** Joins as whoever is signed in. */
  function join(): Promise<void> {
    return run(undefined, async () => {
      onJoined(await accept(token));
    });
  }

  function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (mode === 'register') {
      return run('register', async () => {
        const name = username.trim();
        const accepted = await accept(token, {
          username: name,
          // Trimmed by the server, as it counts white space.
          displayName,
          email: email.trim(),
          password,
        });
        // The account is there and let in. Should signing in fail, the form
        // to go on with is the one to sign in with, the name filled in.
        login = name;
        mode = 'signIn';
        await onSignIn(name, password);
        onJoined(accepted);
      });
    }
    return run('signIn', async () => {
      if (!signedIn) await onSignIn(login.trim(), password);
      signedIn = true;
      onJoined(await accept(token));
    });
  }

  function switchTo(next: 'register' | 'signIn'): void {
    mode = next;
    problem = undefined;
    fieldProblems = {};
  }
</script>

{#snippet invitedTo(found: InvitationPreview)}
  <p class="text-lg font-medium" data-testid="invite-summary">
    {#if found.boardTitle !== undefined}
      {m.invite_summary_board({
        inviter: found.inviterName,
        workspace: found.workspaceName || m.invite_workspace_unnamed(),
        board: found.boardTitle || m.invite_board_untitled(),
      })}
    {:else}
      {m.invite_summary({
        inviter: found.inviterName,
        workspace: found.workspaceName || m.invite_workspace_unnamed(),
      })}
    {/if}
  </p>
{/snippet}

{#snippet fieldProblem(field: 'login' | 'password')}
  {#if fieldProblems[field]}
    <p
      id="account-{field}-problem"
      class="text-sm text-destructive"
      role="alert"
      data-testid="problem-{field}"
    >
      {fieldProblems[field]}
    </p>
  {/if}
{/snippet}

{#snippet unavailable()}
  <div class="grid gap-3 text-center" data-testid="invite-unavailable">
    <h1 class="text-lg font-medium">{m.invite_unavailable_title()}</h1>
    <p class="text-sm text-muted-foreground">{m.invite_unavailable_hint()}</p>
    <Button href={homeHref} variant="outline" class="roomy:min-h-touch">
      {m.invite_to_app()}
    </Button>
  </div>
{/snippet}

<main class="grid min-h-dvh place-items-center p-4">
  <div class="grid w-full max-w-sm gap-4" data-testid="invite-page">
    <p class="text-center text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
    {#await shown}
      <p class="text-center text-muted-foreground" role="status">{m.app_loading()}</p>
    {:then { found, failed }}
      {#if gone || (!found && !failed)}
        {@render unavailable()}
      {:else if !found}
        <p class="text-center" role="alert">{m.invite_load_failed()}</p>
        <Button
          class="roomy:min-h-touch"
          onclick={() => {
            location.reload();
          }}
        >
          {m.app_retry()}
        </Button>
      {:else if signedInAs !== undefined}
        <div class="grid gap-4 text-center">
          {@render invitedTo(found)}
          <p class="text-sm text-muted-foreground">
            {m.invite_joining_as({ name: signedInAs })}
          </p>
          {#if problem}
            <p class="text-sm text-destructive" role="alert">{problem}</p>
          {/if}
          <Button
            class="roomy:min-h-touch"
            disabled={working}
            onclick={() => void join()}
            data-testid="invite-join"
          >
            {working ? m.account_working() : m.invite_join()}
          </Button>
        </div>
      {:else}
        <form
          class="grid gap-4"
          onsubmit={(event) => void submit(event)}
          data-testid={mode === 'register' ? 'invite-register-form' : 'invite-sign-in-form'}
        >
          <div class="text-center">
            {@render invitedTo(found)}
            <h1 class="mt-2 text-sm text-muted-foreground">
              {mode === 'register' ? m.invite_register_title() : m.invite_sign_in_title()}
            </h1>
          </div>

          {#if mode === 'register'}
            <AccountFields
              bind:username
              bind:displayName
              bind:email
              bind:password
              problems={fieldProblems}
            />
          {:else}
            <div class="grid gap-1.5">
              <Label for="account-login">{m.account_login()}</Label>
              <Input
                id="account-login"
                name="login"
                bind:value={login}
                autocomplete="username"
                required
                disabled={signedIn}
                aria-invalid={fieldProblems.login ? 'true' : undefined}
                aria-describedby={fieldProblems.login ? 'account-login-problem' : undefined}
                class="roomy:h-touch roomy:text-base"
              />
              {@render fieldProblem('login')}
            </div>
            <div class="grid gap-1.5">
              <Label for="account-password">{m.account_password()}</Label>
              <Input
                id="account-password"
                name="password"
                type="password"
                bind:value={password}
                autocomplete="current-password"
                required
                disabled={signedIn}
                aria-invalid={fieldProblems.password ? 'true' : undefined}
                aria-describedby={fieldProblems.password ? 'account-password-problem' : undefined}
                class="roomy:h-touch roomy:text-base"
              />
              {@render fieldProblem('password')}
            </div>
          {/if}

          {#if problem}
            <p class="text-sm text-destructive" role="alert">{problem}</p>
          {/if}

          <Button type="submit" class="roomy:min-h-touch" disabled={working}>
            {#if working}
              {m.account_working()}
            {:else}
              {mode === 'register' ? m.invite_register() : m.invite_sign_in()}
            {/if}
          </Button>

          {#if !signedIn}
            <Button
              variant="link"
              class="roomy:min-h-touch"
              disabled={working}
              onclick={() => {
                switchTo(mode === 'register' ? 'signIn' : 'register');
              }}
            >
              {mode === 'register' ? m.invite_to_sign_in() : m.invite_to_register()}
            </Button>
          {/if}
        </form>
      {/if}
    {/await}
  </div>
</main>
