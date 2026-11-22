<!--
  The page the link mailed to an account's new address opens
  (`/confirm-email#<token>`): it changes the address by the token, whoever
  holds the link and whether or not this browser is signed in, and says what
  came of it. A link that changes nothing says so: asking again is done in the
  settings, with the account's password.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError } from '$lib/state/index.ts';

  import { problemOf } from './problems.ts';

  interface Props {
    /** The change by the link's token, sent once; `undefined` for a link without a token. */
    confirming: Promise<void> | undefined;
    homeHref: string;
  }

  let { confirming, homeHref }: Props = $props();

  /** The refusal of a link without a token, which changes nothing. */
  const NO_TOKEN = new Error('the link has no token');
  /** What the page waits for. */
  const shown = $derived(confirming ?? Promise.reject(NO_TOKEN));

  /** Whether the link itself changes nothing, and not the request that failed. */
  function linkRefused(error: unknown): boolean {
    return (
      error === NO_TOKEN ||
      (isBackendError(error) && error.errors.some((entry) => entry.pointer === '/token'))
    );
  }
</script>

<main class="grid min-h-dvh place-items-center p-4">
  <div class="grid w-full max-w-sm gap-4" data-testid="confirm-email">
    <div class="text-center">
      <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
      <h1 class="mt-2 text-lg font-medium">{m.confirm_email_title()}</h1>
    </div>
    {#await shown}
      <p role="status">{m.confirm_email_working()}</p>
    {:then}
      <p role="status" data-testid="confirm-email-done">{m.confirm_email_done()}</p>
    {:catch error}
      <p role="alert">{linkRefused(error) ? m.confirm_email_invalid() : problemOf(error)}</p>
    {/await}
    <a href={homeHref} class="text-center text-sm underline underline-offset-4"
      >{m.confirm_email_go()}</a
    >
  </div>
</main>
