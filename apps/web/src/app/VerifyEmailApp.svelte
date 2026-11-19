<script lang="ts">
  import { acceptedRoute } from './accepted-route.ts';
  import { startEmailVerification } from './backend.ts';
  import VerifyEmailPage from '$lib/features/account/VerifyEmailPage.svelte';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import type { Confirmed } from '$lib/state/email-verification.ts';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();
  const verification = startEmailVerification();

  /**
   * Confirms, and goes to where the account was let in: the welcome board of an
   * organization it started, the board or the wiki of a guest's link, or the
   * workspace. The token leaves the address and the history.
   */
  async function complete(token: string, password?: string): Promise<Confirmed> {
    const client = await verification;
    const confirmed = await client.complete(token, password);
    history.replaceState(null, '', location.pathname);
    let route = acceptedRoute(confirmed);
    if (confirmed.organizationStarted) {
      // The account is in: no welcome board is no reason to keep it out.
      const boardId = await client.welcome(confirmed.workspaceId).catch(() => undefined);
      route = { workspaceId: confirmed.workspaceId, boardId };
    }
    location.replace(router.href(route));
    return confirmed;
  }

  /** Sends the confirmation of the token in the address, if it has one: once for each. */
  function confirmFromAddress(): Promise<Confirmed> | undefined {
    const token = location.hash.slice(1);
    return token ? complete(token) : undefined;
  }

  let confirming = $state.raw(confirmFromAddress());
  function onhashchange(): void {
    confirming = confirmFromAddress();
  }

  /** Confirms again by the token in the address, with the registration's password. */
  function onPassword(password: string): void {
    const token = location.hash.slice(1);
    confirming = token ? complete(token, password) : undefined;
  }

  async function resend(email: string): Promise<void> {
    await (await verification).resend({ email }, getLocale());
  }
</script>

<svelte:window {onhashchange} />
<VerifyEmailPage {confirming} homeHref={router.href({})} {resend} {onPassword} />
