<!--
  Signing in, and making an account, on a backend that has accounts (the
  server). The page the app shows while nobody is signed in.

  The two forms share the page: a link turns one into the other. Making an
  account does not sign in on the server, so the page signs in right after
  with what was typed; if that fails, the page is the sign-in form. The
  length of a password is the server's to check (it counts characters, a
  browser's `minlength` counts UTF-16 units). A refusal is told by its status and reason, never by
  the server's own words (`Problem` in `lib/state/backend.ts`). Where the
  server names the members it refuses (`errors`), each field says what is wrong
  with it, beside itself; any other refusal is one line above the button.

  An organization whose people sign in through its provider has a button
  for that above the form: the organization the page's address names
  (`/login?org=…`), or the only one of the server. A sign-in through the
  provider that went wrong comes back to this page with the name of what
  went wrong in `oidc_error`, which is told as a refusal is.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import {
    isBackendError,
    type InvalidInput,
    type LoginOrganization,
    type Registration,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  interface Props {
    onSignIn: (login: string, password: string) => Promise<void>;
    onRegister: (registration: Registration) => Promise<void>;
    /** The organization the page is for (`Account.loginOrganization`). */
    organizationOf?: (orgId: string | undefined) => Promise<LoginOrganization | undefined>;
    /** Where the browser goes to sign in through a provider (`Account.oidcStartUrl`). */
    oidcStartUrl?: (orgId: string) => string;
  }

  let { onSignIn, onRegister, organizationOf, oidcStartUrl }: Props = $props();

  /** What the page's address says: the organization, and what went wrong. */
  const address = new URLSearchParams(location.search);
  // An empty `org` names none: the server's only organization, if it has one.
  const askedOrg = address.get('org') || undefined;

  /** The organization with a provider, once read; undefined while there is none. */
  const organization = $derived(
    organizationOf?.(askedOrg).then(
      (found) => (found?.oidc ? found : undefined),
      () => undefined,
    ),
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
      case 'rate-limited':
        return m.account_rate_limited();
      default:
        return m.account_oidc_failed();
    }
  }
  const oidcError = address.get('oidc_error');

  let mode = $state<'signIn' | 'register'>('signIn');
  let login = $state('');
  let username = $state('');
  let displayName = $state('');
  let email = $state('');
  let password = $state('');
  let token = $state('');
  let working = $state(false);
  let problem = $state<string | undefined>(oidcError ? oidcProblemOf(oidcError) : undefined);

  type Field = 'login' | 'username' | 'displayName' | 'email' | 'password' | 'token';

  /** What is wrong with each field, where the server said so of the field. */
  let fieldProblems = $state.raw<Partial<Record<Field, string>>>({});

  /**
   * The field of each form that a member of its request is typed into, by the
   * member's JSON Pointer. Signing in sends what was typed as the name and as
   * the email address.
   */
  const FIELDS: Record<'signIn' | 'register', Record<string, Field>> = {
    signIn: { '/username': 'login', '/email': 'login', '/password': 'password' },
    register: {
      '/username': 'username',
      '/displayName': 'displayName',
      '/email': 'email',
      '/password': 'password',
      '/token': 'token',
    },
  };

  /** The words for what is wrong with a field. */
  function fieldProblemOf(entry: InvalidInput, field: Field): string {
    switch (entry.reason) {
      case 'required':
        return m.account_field_required();
      case 'too_short':
        return field === 'password' ? m.account_password_too_short() : m.account_field_invalid();
      case 'too_long':
        return field === 'displayName'
          ? m.account_display_name_too_long()
          : m.account_field_too_long();
      case 'invalid_characters':
        return m.account_display_name_invalid_characters();
      case 'invalid_format':
        return field === 'email' ? m.account_email_invalid() : m.account_field_invalid();
      case 'username_taken':
        return m.account_username_taken();
      case 'email_taken':
        return m.account_email_taken();
      case 'signup_token_required':
        return m.account_token_required();
      case 'signup_token_invalid':
        return m.account_token_invalid();
      default:
        return m.account_field_invalid();
    }
  }

  /**
   * What the server said of the fields of the form `of`, the first word about
   * each, and whether it also refused something that is no field of the form.
   */
  function fieldProblemsOf(
    error: unknown,
    of: 'signIn' | 'register',
  ): { fields: typeof fieldProblems; other: boolean } {
    const fields: typeof fieldProblems = {};
    let other = false;
    if (!isBackendError(error)) return { fields, other };
    for (const entry of error.errors) {
      const field = entry.pointer === undefined ? undefined : FIELDS[of][entry.pointer];
      if (field) fields[field] ??= fieldProblemOf(entry, field);
      else other = true;
    }
    return { fields, other };
  }

  /** The fields in the order the forms show them, for finding the first that is wrong. */
  const FIELD_ORDER: readonly Field[] = [
    'login',
    'username',
    'displayName',
    'email',
    'password',
    'token',
  ];

  /** The words for a refusal. */
  function problemOf(error: unknown): string {
    if (!isBackendError(error)) return m.account_failed();
    switch (error.reason) {
      case 'invalid_credentials':
        return m.account_invalid_credentials();
      case 'session_not_kept':
        return m.account_session_not_kept();
      case 'username_taken':
        return m.account_username_taken();
      case 'email_taken':
        return m.account_email_taken();
      case 'signup_token_required':
        return m.account_token_required();
      case 'signup_token_invalid':
        return m.account_token_invalid();
      default:
        break;
    }
    if (error.status === 429) return m.account_rate_limited();
    if (error.status === 401) return m.account_invalid_credentials();
    return m.account_failed();
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
          username: username.trim(),
          // Trimmed by the server, as it counts white space.
          displayName,
          email: email.trim(),
          password,
          token: token.trim(),
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
      const { fields, other } = fieldProblemsOf(error, mode);
      fieldProblems = fields;
      const first = FIELD_ORDER.find((field) => fields[field] !== undefined);
      // One line for the whole form when no field has a word of its own, and
      // when something was refused that is no field: the fields are not all of it.
      if (first === undefined) problem = problemOf(error);
      else if (other) problem = m.account_failed();
      // The first field that is wrong is where to go on.
      if (first !== undefined) {
        await tick();
        document.getElementById(`account-${first}`)?.focus();
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

{#snippet fieldProblem(field: Field)}
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

    {#if mode === 'signIn' && oidcStartUrl}
      {#await organization then found}
        {#if found}
          <Button
            href={oidcStartUrl(found.id)}
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
    {:else}
      <div class="grid gap-1.5">
        <Label for="account-username">{m.account_username()}</Label>
        <Input
          id="account-username"
          name="username"
          bind:value={username}
          autocomplete="username"
          required
          aria-invalid={fieldProblems.username ? 'true' : undefined}
          aria-describedby={fieldProblems.username ? 'account-username-problem' : undefined}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('username')}
      </div>
      <div class="grid gap-1.5">
        <Label for="account-displayName">{m.account_display_name()}</Label>
        <Input
          id="account-displayName"
          name="displayName"
          bind:value={displayName}
          autocomplete="name"
          aria-invalid={fieldProblems.displayName ? 'true' : undefined}
          aria-describedby={fieldProblems.displayName
            ? 'account-displayName-problem account-displayName-hint'
            : 'account-displayName-hint'}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('displayName')}
        <p id="account-displayName-hint" class="text-xs text-muted-foreground">
          {m.account_display_name_hint()}
        </p>
      </div>
      <div class="grid gap-1.5">
        <Label for="account-email">{m.account_email()}</Label>
        <Input
          id="account-email"
          name="email"
          type="email"
          bind:value={email}
          autocomplete="email"
          required
          aria-invalid={fieldProblems.email ? 'true' : undefined}
          aria-describedby={fieldProblems.email ? 'account-email-problem' : undefined}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('email')}
      </div>
    {/if}

    <div class="grid gap-1.5">
      <Label for="account-password">{m.account_password()}</Label>
      <Input
        id="account-password"
        name="password"
        type="password"
        bind:value={password}
        autocomplete={mode === 'signIn' ? 'current-password' : 'new-password'}
        required
        aria-invalid={fieldProblems.password ? 'true' : undefined}
        aria-describedby={fieldProblems.password
          ? 'account-password-problem'
          : mode === 'register'
            ? 'account-password-hint'
            : undefined}
        class="roomy:h-touch roomy:text-base"
      />
      {@render fieldProblem('password')}
      <!-- What is wrong with the password says what the hint would say again. -->
      {#if mode === 'register' && !fieldProblems.password}
        <p id="account-password-hint" class="text-xs text-muted-foreground">
          {m.account_password_hint()}
        </p>
      {/if}
    </div>

    {#if mode === 'register'}
      <div class="grid gap-1.5">
        <Label for="account-token">{m.account_token()}</Label>
        <Input
          id="account-token"
          name="token"
          bind:value={token}
          autocomplete="off"
          aria-invalid={fieldProblems.token ? 'true' : undefined}
          aria-describedby={fieldProblems.token
            ? 'account-token-problem account-token-hint'
            : 'account-token-hint'}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('token')}
        <p id="account-token-hint" class="text-xs text-muted-foreground">
          {m.account_token_hint()}
        </p>
      </div>
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
  </form>
</main>
