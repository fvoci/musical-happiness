<script lang="ts">
  import { startEmailChange } from './backend.ts';
  import { forgetFragmentToken, takeFragmentToken } from '$lib/fragment-token.ts';
  import ConfirmEmailPage from '$lib/features/account/ConfirmEmailPage.svelte';
  import { isBackendError } from '$lib/state/index.ts';
  import type { Router } from './router.svelte.ts';

  const SCOPE = 'confirm-email';

  let { router }: { router: Router } = $props();
  const change = startEmailChange();

  /** Changes the address by the token; it then leaves `sessionStorage` too. */
  async function confirm(token: string): Promise<void> {
    try {
      await (await change).confirm(token);
      forgetFragmentToken(SCOPE);
    } catch (error) {
      if (isBackendError(error) && error.errors.some((entry) => entry.pointer === '/token')) {
        forgetFragmentToken(SCOPE);
      }
      throw error;
    }
  }

  /** The link as it was opened: each opening is new, also of the same link again. */
  let opened = $state.raw({ token: takeFragmentToken(SCOPE) });
  function onhashchange(): void {
    if (location.hash.length > 1) opened = { token: takeFragmentToken(SCOPE) };
  }
</script>

<svelte:window {onhashchange} />
{#key opened}
  <ConfirmEmailPage token={opened.token} {confirm} homeHref={router.href({})} />
{/key}
