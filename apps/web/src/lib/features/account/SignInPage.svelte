<!--
  Signing in, and making an account, on a backend that has accounts (the
  server). The page the app shows while nobody is signed in.

  The two forms share the page: a link turns one into the other. Making an
  account does not sign in on the server, so the page signs in right after
  with what was typed; if that fails, the page is the sign-in form. The
  length of a password is the server's to check (it counts characters, a
  browser's `minlength` counts UTF-16 units). A refusal is told by its kind and reason, never by
  the server's own words (`docs/contract-assumptions.md`, 1).
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError, type Registration } from '$lib/state/index.ts';
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

  const FIELD_LABELS: Record<string, () => string> = {
    username: () => m.account_username(),
    email: () => m.account_email(),
    password: () => m.account_password(),
    token: () => m.account_token(),
  };

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
    const field = error.field === undefined ? undefined : FIELD_LABELS[error.field];
    if (error.status === 400 && field) return m.account_bad_field({ field: field() });
    return m.account_failed();
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (working) return;
    working = true;
    problem = undefined;
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
      problem = problemOf(error);
    } finally {
      working = false;
    }
  }

  function switchTo(next: 'signIn' | 'register'): void {
    mode = next;
    problem = undefined;
  }
</script>

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
          class="roomy:h-touch roomy:text-base"
        />
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
          class="roomy:h-touch roomy:text-base"
        />
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
          class="roomy:h-touch roomy:text-base"
        />
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
        class="roomy:h-touch roomy:text-base"
      />
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
          class="roomy:h-touch roomy:text-base"
        />
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
