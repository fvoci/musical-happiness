<!--
  The page the confirmation link opens (`/verify-email#<token>`): it confirms
  the registration at once, which signs this browser in, and goes to where the
  account was let in. A link that confirms nothing says why, and offers a new
  one for the address typed in.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError } from '$lib/state/index.ts';
  import type { Confirmed } from '$lib/state/email-verification.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  import { problemOf } from './problems.ts';

  interface Props {
    /** The token from the link's fragment; empty for none. */
    token: string;
    homeHref: string;
    /** Confirms by the token, and goes on to where the account was let in. */
    complete: (token: string) => Promise<Confirmed>;
    /** Mails a new link for the registration of the address. */
    resend: (email: string) => Promise<void>;
  }

  let { token, homeHref, complete, resend }: Props = $props();

  /** The confirmation, sent once for the token the page was opened with. */
  const confirming = $derived(
    token ? complete(token) : Promise.reject(new Error('the link has no token')),
  );

  let email = $state('');
  let working = $state(false);
  let resent = $state(false);
  let problem = $state<string | undefined>(undefined);

  /** Whether the link itself confirms nothing (none, ended, used, replaced). */
  function linkRefused(error: unknown): boolean {
    return !isBackendError(error) || error.errors.some((entry) => entry.pointer === '/token');
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (working) return;
    working = true;
    resent = false;
    problem = undefined;
    try {
      await resend(email.trim());
      resent = true;
    } catch (error) {
      problem = problemOf(error);
    } finally {
      working = false;
    }
  }
</script>

<main class="grid min-h-dvh place-items-center p-4">
  <div class="grid w-full max-w-sm gap-4">
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <h1 class="mt-2 text-lg font-medium">{m.verify_title()}</h1>
    </div>
    {#await confirming}
      <p role="status" data-testid="verify-status">{m.verify_working()}</p>
    {:then}
      <p role="status" data-testid="verify-status">{m.verify_done()}</p>
    {:catch error}
      {#if linkRefused(error)}
        <p role="alert" data-testid="verify-problem">{m.verify_invalid()}</p>
        <form
          class="grid gap-4"
          onsubmit={(event) => void submit(event)}
          data-testid="verify-request-form"
        >
          <div class="grid gap-1.5">
            <Label for="verify-email">{m.account_email()}</Label>
            <Input
              id="verify-email"
              type="email"
              autocomplete="email"
              bind:value={email}
              required
              disabled={working}
              class="roomy:h-touch roomy:text-base"
            />
          </div>
          <Button type="submit" disabled={working} class="roomy:min-h-touch">
            {m.verify_request_submit()}
          </Button>
          <p class="text-sm" role="status">{resent ? m.verify_resent() : ''}</p>
          {#if problem}
            <p class="text-sm text-destructive" role="alert">{problem}</p>
          {/if}
        </form>
      {:else}
        <p role="alert" data-testid="verify-problem">{problemOf(error)}</p>
      {/if}
    {/await}
    <a href={homeHref} class="text-center text-sm underline underline-offset-4"
      >{m.reset_sign_in()}</a
    >
  </div>
</main>
