<script lang="ts">
  import { startEmailVerification } from './backend.ts';
  import VerifyEmailPage from '$lib/features/account/VerifyEmailPage.svelte';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import type { Confirmed } from '$lib/state/email-verification.ts';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();
  const verification = startEmailVerification();

  let token = $state(location.hash.slice(1));
  function onhashchange(): void {
    token = location.hash.slice(1);
  }

  /**
   * Confirms, and goes to where the account was let in: the welcome board of an
   * organization it started (its tour), the board of a guest's link, or the
   * workspace. The token leaves the address and the history.
   */
  async function complete(token: string): Promise<Confirmed> {
    const client = await verification;
    const confirmed = await client.complete(token);
    history.replaceState(null, '', location.pathname);
    let boardId = confirmed.boardId;
    if (confirmed.organizationStarted) {
      // The account is in: no welcome board is no reason to keep it out.
      boardId = await client.welcome(confirmed.workspaceId).catch(() => undefined);
    }
    location.replace(router.href({ workspaceId: confirmed.workspaceId, boardId }));
    return confirmed;
  }

  async function resend(email: string): Promise<void> {
    await (await verification).resend({ email }, getLocale());
  }
</script>

<svelte:window {onhashchange} />
{#key token}
  <VerifyEmailPage {token} homeHref={router.href({})} {complete} {resend} />
{/key}
