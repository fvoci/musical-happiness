<script lang="ts">
  import { startPasswordRecovery } from './backend.ts';
  import ResetPasswordPage from '$lib/features/account/ResetPasswordPage.svelte';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();
  const recovery = startPasswordRecovery();
  let token = $state(location.hash.slice(1));
  function onhashchange(): void {
    token = location.hash.slice(1);
  }

  async function complete(token: string, password: string): Promise<void> {
    await (await recovery).complete(token, password);
  }
</script>

<svelte:window {onhashchange} />
{#key token}
  <ResetPasswordPage {token} homeHref={router.href({})} {complete} />
{/key}
