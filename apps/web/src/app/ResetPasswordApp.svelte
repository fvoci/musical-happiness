<script lang="ts">
  import { untrack } from 'svelte';

  import { startPasswordRecovery } from './backend.ts';
  import { forgetFragmentToken, takeFragmentToken } from './fragment-token.ts';
  import RequestPasswordPage from '$lib/features/account/RequestPasswordPage.svelte';
  import ResetPasswordPage from '$lib/features/account/ResetPasswordPage.svelte';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { isBackendError } from '$lib/state/index.ts';
  import { emailOfForgotPassword } from './recovery-links.ts';
  import type { Router } from './router.svelte.ts';

  const SCOPE = 'reset-password';

  let { router }: { router: Router } = $props();
  const recovery = startPasswordRecovery();
  const isForgot = $derived(router.route.kind === 'passwordForgot');
  const available = $derived(
    isForgot ? recovery.then((client) => client.available()) : Promise.resolve(false),
  );
  async function request(email: string): Promise<void> {
    await (await recovery).request(email, getLocale());
  }
  let token = $state(
    untrack(() => (router.route.kind === 'passwordForgot' ? '' : takeFragmentToken(SCOPE))),
  );
  function onhashchange(): void {
    if (!isForgot && location.hash.length > 1) {
      token = takeFragmentToken(SCOPE);
    }
  }

  async function complete(secret: string, password: string): Promise<void> {
    try {
      await (await recovery).complete(secret, password);
      forgetFragmentToken(SCOPE);
    } catch (error) {
      if (isBackendError(error) && error.errors.some((entry) => entry.pointer === '/token')) {
        forgetFragmentToken(SCOPE);
      }
      throw error;
    }
  }
</script>

<svelte:window {onhashchange} />
{#if isForgot}
  <RequestPasswordPage
    {available}
    {request}
    email={emailOfForgotPassword(location.hash)}
    homeHref={router.href({})}
  />
{:else}
  {#key token}
    <ResetPasswordPage {token} homeHref={router.href({})} {complete} />
  {/key}
{/if}
