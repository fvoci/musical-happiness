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
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError, type InvalidInput, type Registration } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  interface Props {
    onSignIn: (login: string, password: string) => Promise<void>;
    onRegister: (registration: Registration) => Promise<void>;
  }

  let { onSignIn, onRegister }: Props = $props();

  let mode = $state<'signIn' | 'register'>('signIn');
  let login = $state('');
  let username = $state('');
  let email = $state('');
  let password = $state('');
  let token = $state('');
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);

  type Field = 'login' | 'username' | 'email' | 'password' | 'token';

  /** What is wrong with each field, where the server said so of the field. */
  let fieldProblems = $state<Partial<Record<Field, string>>>({});

  /**
   * The field of each form that a member of its request is typed into, by the
   * member's JSON Pointer. Signing in sends what was typed as the name and as
   * the email address.
   */
  const FIELDS: Record<'signIn' | 'register', Record<string, Field>> = {
    signIn: { '/username': 'login', '/email': 'login', '/password': 'password' },
    register: {
      '/username': 'username',
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
        return m.account_field_too_long();
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

  /** What the server said of the fields of the form `of`; the first word about each. */
  function fieldProblemsOf(error: unknown, of: 'signIn' | 'register'): typeof fieldProblems {
    const problems: typeof fieldProblems = {};
    if (!isBackendError(error)) return problems;
    for (const entry of error.errors) {
      const field = entry.pointer === undefined ? undefined : FIELDS[of][entry.pointer];
      if (field) problems[field] ??= fieldProblemOf(entry, field);
    }
    return problems;
  }

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
      // `mode` is the form the refused request was sent from.
      fieldProblems = fieldProblemsOf(error, mode);
      // One line for the whole form only when no field has a word of its own.
      problem = Object.keys(fieldProblems).length > 0 ? undefined : problemOf(error);
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
        aria-describedby={fieldProblems.password ? 'account-password-problem' : undefined}
        class="roomy:h-touch roomy:text-base"
      />
      {@render fieldProblem('password')}
      {#if mode === 'register'}
        <p class="text-xs text-muted-foreground">{m.account_password_hint()}</p>
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
          aria-describedby={fieldProblems.token ? 'account-token-problem' : undefined}
          class="roomy:h-touch roomy:text-base"
        />
        {@render fieldProblem('token')}
        <p class="text-xs text-muted-foreground">{m.account_token_hint()}</p>
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
      onclick={() => {
        switchTo(mode === 'signIn' ? 'register' : 'signIn');
      }}
    >
      {mode === 'signIn' ? m.account_to_register() : m.account_to_sign_in()}
    </Button>
  </form>
</main>
