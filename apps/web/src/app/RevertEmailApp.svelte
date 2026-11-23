<script lang="ts">
  import { startEmailChange } from './backend.ts';
  import RevertEmailPage from '$lib/features/account/RevertEmailPage.svelte';
  import { forgotPasswordHref } from './recovery-links.ts';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();
  const change = startEmailChange();

  /** Gives the address back by the token; it then leaves the address bar and the history. */
  async function revert(token: string): Promise<string | undefined> {
    const email = await (await change).revert(token);
    history.replaceState(null, '', location.pathname);
    return email;
  }

  /** The page that asks for a reset link, filled in with the address given back. */
  function resetHref(email: string | undefined): string {
    return forgotPasswordHref(router.href({}), email);
  }

  /** The link as it was opened: each opening is new, also of the same link again. */
  let opened = $state.raw({ token: location.hash.slice(1) });
  function onhashchange(): void {
    // The token that was just used left the address: what it came to stays said.
    if (location.hash.length > 1) opened = { token: location.hash.slice(1) };
  }
</script>

<svelte:window {onhashchange} />
{#key opened}
  <RevertEmailPage token={opened.token} {revert} homeHref={router.href({})} {resetHref} />
{/key}
