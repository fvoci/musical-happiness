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

  /** Sends the token in the address, if it has one: once for each. */
  function confirmFromAddress(): Promise<void> | undefined {
    const token = location.hash.slice(1);
    return token ? confirm(token) : undefined;
  }

  let confirming = $state.raw(confirmFromAddress());
  function onhashchange(): void {
    // The token that was just used left the address: what it came to stays said.
    if (location.hash.length > 1) confirming = confirmFromAddress();
  }
</script>

<svelte:window {onhashchange} />
<ConfirmEmailPage {confirming} homeHref={router.href({})} />
