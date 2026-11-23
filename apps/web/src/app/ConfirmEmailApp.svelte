<script lang="ts">
  import { startEmailChange } from './backend.ts';
  import ConfirmEmailPage from '$lib/features/account/ConfirmEmailPage.svelte';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();
  const change = startEmailChange();

  /** Changes the address by the token; it then leaves the address bar and the history. */
  async function confirm(token: string): Promise<void> {
    await (await change).confirm(token);
    history.replaceState(null, '', location.pathname);
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
  <ConfirmEmailPage token={opened.token} {confirm} homeHref={router.href({})} />
{/key}
