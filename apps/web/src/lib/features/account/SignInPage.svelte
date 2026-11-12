<!--
  Signing in, and starting an organization with an account of one's own, on a backend
  that has accounts (the server). The page the app shows while nobody is
  signed in.

  The two forms share the page: a link turns one into the other. Making an
  account does not sign in on the server, so the page signs in right after
  with what was typed; if that fails, the page is the sign-in form. A refusal
  is told as `problems.ts` says.

  Where the server takes nobody without an invitation (it has its organization), the
  page offers no form to make an account, and says what to ask for instead.

  An organization whose people sign in through its provider has a button
  for that above the form: the organization the page's address names
  (`/login?org=…`), or the only one of the server. A sign-in through the
  provider that went wrong comes back to this page with the name of what
  went wrong in `oidc_error`, which is told as a refusal is.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError, type LoginOptions, type Registration } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  import AccountFields from './AccountFields.svelte';
  import {
    accountFieldsUnder,
    formProblemsOf,
    problemOf,
    type AccountField,
    type FieldProblems,
  } from './problems.ts';

  interface Props {
    onSignIn: (login: string, password: string) => Promise<void>;
    onRegister: (registration: Registration) => Promise<void>;
    /** What the page shows of the server (`Account.loginOptions`). */
    optionsOf?: (orgId: string | undefined) => Promise<LoginOptions>;
    /**
     * Starts a sign-in through the provider and sends the browser to it
     * (`Account.startOidc`); a refusal of the start rejects.
     */
    onOidcStart?: (orgId: string) => Promise<void>;
  }

  let { onSignIn, onRegister, optionsOf, onOidcStart }: Props = $props();

  /** What the page's address says: the organization, and what went wrong. */
  const address = new URLSearchParams(location.search);
  // An empty `org` names none: the server's only organization, if it has one.
  const askedOrg = address.get('org') || undefined;

  /** What the server says of itself, once read; undefined if it could not be. */
  const options = $derived(optionsOf?.(askedOrg).catch(() => undefined));
  /** The organization with a provider, once read; undefined while there is none. */
  const organization = $derived(
    options?.then((read) => (read?.organization?.oidc ? read.organization : undefined)),
  );

  /** The words for what went wrong with a sign-in through a provider. */
  function oidcProblemOf(name: string): string {
    switch (name) {
      case 'oidc-state-invalid':
        return m.account_oidc_state_invalid();
      case 'oidc-denied':
        return m.account_oidc_denied();
      case 'oidc-email-missing':
        return m.account_oidc_email_missing();
      case 'oidc-email-unverified':
        return m.account_oidc_email_unverified();
      case 'oidc-email-taken':
        return m.account_oidc_email_taken();
      case 'oidc-join-closed':
        return m.account_oidc_join_closed();
      default:
        return m.account_oidc_failed();
    }
  }
  const oidcError = address.get('oidc_error');

  let mode = $state<'signIn' | 'register'>('signIn');
  let login = $state('');
  let organizationName = $state('');
  let username = $state('');
  let displayName = $state('');
  let email = $state('');
  let password = $state('');
  let working = $state(false);
  let problem = $state<string | undefined>(oidcError ? oidcProblemOf(oidcError) : undefined);

  /** What is wrong with each field, where the server said so of the field. */
  let fieldProblems = $state.raw<FieldProblems>({});

  /**
   * The field of each form that a member of its request is typed into, by the
   * member's JSON Pointer. Signing in sends what was typed as the name and as
   * the email address.
   */
  const FIELDS: Record<'signIn' | 'register', Record<string, AccountField>> = {
    signIn: { '/username': 'login', '/email': 'login', '/password': 'password' },
    register: { ...accountFieldsUnder(''), '/organizationName': 'organizationName' },
  };

  /** Starts a sign-in through the organization's provider; says why it did not. */
  async function startOidc(orgId: string): Promise<void> {
    if (working || !onOidcStart) return;
    working = true;
    problem = undefined;
    try {
      // Started, the browser is leaving for the provider: the page stays busy,
      // so that a second press starts no second sign-in in place of this one.
      await onOidcStart(orgId);
    } catch (error) {
      problem =
        isBackendError(error) && error.reason === 'oidc_unavailable'
          ? m.account_oidc_failed()
          : problemOf(error);
      working = false;
    }
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (working) return;
    working = true;
    problem = undefined;
    fieldProblems = {};
    try {
      if (mode === 'register') {
        await onRegister({
          organizationName: organizationName.trim(),
          username: username.trim(),
          // Trimmed by the server, as it counts white space.
          displayName,
          email: email.trim(),
          password,
        });
        // The account is there now. Should signing in fail, the form to try
        // again is the one to sign in with, the name filled in.
        login = username.trim();
        mode = 'signIn';
        await onSignIn(login, password);
      } else {
        await onSignIn(login.trim(), password);
      }
    } catch (error) {
      // `mode` is the form the refused request was sent from: the link that
      // turns one form into the other is off while a request is under way.
      const said = formProblemsOf(error, FIELDS[mode]);
      fieldProblems = said.fields;
      problem = said.problem;
      // The first field that is wrong is where to go on.
      if (said.first !== undefined) {
        await tick();
        document.getElementById(`account-${said.first}`)?.focus();
      }
    } finally {
      working = false;
    }
  }

  function switchTo(next: 'signIn' | 'register'): void {
    mode = next;
    problem = undefined;
    fieldProblems = {};
  }
</script>

{#snippet fieldProblem(field: 'login' | 'password' | 'organizationName')}
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

<main class="grid min-h-dvh place-items-center p-4">
  <form
    class="grid w-full max-w-sm gap-4"
    onsubmit={(event) => void submit(event)}
    data-testid={mode === 'signIn' ? 'sign-in-form' : 'register-form'}
  >
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <h1 class="mt-2 text-lg font-medium">
        {mode === 'signIn' ? m.account_sign_in_title() : m.account_register_title()}
      </h1>
    </div>

    {#if mode === 'signIn' && onOidcStart}
      {#await organization then found}
        {#if found}
          <Button
            type="button"
            onclick={() => void startOidc(found.id)}
            disabled={working}
            variant="outline"
            class="roomy:min-h-touch"
            data-testid="oidc-continue"
          >
            {found.name
              ? m.account_oidc_continue({ name: found.name })
              : m.account_oidc_continue_unnamed()}
          </Button>
          <p class="text-center text-sm text-muted-foreground">{m.account_oidc_or()}</p>
        {/if}
      {/await}
    {/if}

    {#if mode === 'signIn'}
      <div class="grid gap-1.5">
        <Label for="account-login">{m.account_login()}</Label>
        <Input
          id="account-login"
          name="login"
          bind:value={login}
          autocomplete="username"
          required
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
          aria-invalid={fieldProblems.password ? 'true' : undefined}
          aria-describedby={fieldProblems.password ? 'account-password-problem' : undefined}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('password')}
      </div>
    {:else}
      <div class="grid gap-1.5">
        <Label for="account-organizationName">{m.account_organization_name()}</Label>
        <Input
          id="account-organizationName"
          name="organizationName"
          bind:value={organizationName}
          autocomplete="organization"
          required
          aria-invalid={fieldProblems.organizationName ? 'true' : undefined}
          aria-describedby={fieldProblems.organizationName
            ? 'account-organizationName-problem account-organizationName-hint'
            : 'account-organizationName-hint'}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('organizationName')}
        <p id="account-organizationName-hint" class="text-xs text-muted-foreground">
          {m.account_organization_name_hint()}
        </p>
      </div>
      <AccountFields
        bind:username
        bind:displayName
        bind:email
        bind:password
        problems={fieldProblems}
      />
    {/if}

    {#if problem}
      <p class="text-sm text-destructive" role="alert">{problem}</p>
    {/if}

    <Button type="submit" class="roomy:min-h-touch" disabled={working}>
      {#if working}
        {m.account_working()}
      {:else}
        {mode === 'signIn' ? m.account_sign_in() : m.account_register()}
      {/if}
    </Button>

    <!-- Until the server has said whether it takes an account, neither is offered. -->
    {#await options then read}
      {#if mode === 'register' || read?.registrationOpen !== false}
        <Button
          variant="link"
          class="roomy:min-h-touch"
          disabled={working}
          onclick={() => {
            switchTo(mode === 'signIn' ? 'register' : 'signIn');
          }}
        >
          {mode === 'signIn' ? m.account_to_register() : m.account_to_sign_in()}
        </Button>
      {:else}
        <p class="text-center text-sm text-muted-foreground" data-testid="invitation-needed">
          {m.account_invitation_needed()}
        </p>
      {/if}
    {/await}
  </form>
</main>
