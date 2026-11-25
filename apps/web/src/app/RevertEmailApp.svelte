<script lang="ts">
  import { startEmailChange } from './backend.ts';
  import { forgetFragmentToken, takeFragmentToken } from '$lib/fragment-token.ts';
  import RevertEmailPage from '$lib/features/account/RevertEmailPage.svelte';
  import { isBackendError } from '$lib/state/index.ts';
  import { forgotPasswordHref } from './recovery-links.ts';
  import type { Router } from './router.svelte.ts';

  const SCOPE = 'revert-email';

  let { router }: { router: Router } = $props();
  const change = startEmailChange();

  /** Gives the address back by the token; it then leaves `sessionStorage` too. */
  async function revert(token: string): Promise<string | undefined> {
    try {
      const email = await (await change).revert(token);
      forgetFragmentToken(SCOPE);
      return email;
    } catch (error) {
      if (isBackendError(error) && error.errors.some((entry) => entry.pointer === '/token')) {
        forgetFragmentToken(SCOPE);
      }
      throw error;
    }
  }

  /** The page that asks for a reset link, filled in with the address given back. */
  function resetHref(email: string | undefined): string {
    return forgotPasswordHref(router.href({}), email);
  }

  /** The link as it was opened: each opening is new, also of the same link again. */
  let opened = $state.raw({ token: takeFragmentToken(SCOPE) });
  function onhashchange(): void {
    if (location.hash.length > 1) opened = { token: takeFragmentToken(SCOPE) };
  }
</script>

<svelte:window {onhashchange} />
{#key opened}
  <RevertEmailPage token={opened.token} {revert} homeHref={router.href({})} {resetHref} />
{/key}
