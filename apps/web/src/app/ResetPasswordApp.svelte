<script lang="ts">
  import { startPasswordRecovery } from './backend.ts';
  import RequestPasswordPage from '$lib/features/account/RequestPasswordPage.svelte';
  import ResetPasswordPage from '$lib/features/account/ResetPasswordPage.svelte';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import type { Router } from './router.svelte.ts';

  let { router }: { router: Router } = $props();
  const recovery = startPasswordRecovery();
  const available = $derived(
    router.route.passwordForgot
      ? recovery.then((client) => client.available())
      : Promise.resolve(false),
  );
  async function request(email: string): Promise<void> {
    await (await recovery).request(email, getLocale());
  }
  let token = $state(location.hash.slice(1));
  function onhashchange(): void {
    token = location.hash.slice(1);
  }

  async function complete(token: string, password: string): Promise<void> {
    await (await recovery).complete(token, password);
  }
</script>

<svelte:window {onhashchange} />
{#if router.route.passwordForgot}
  <RequestPasswordPage {available} {request} homeHref={router.href({})} />
{:else}
  {#key token}
    <ResetPasswordPage {token} homeHref={router.href({})} {complete} />
  {/key}
{/if}
