<!--
  The provider's pending settings: saved aside while single sign-on is required,
  and used by nobody until a sign-in through them succeeds, which applies them.
  The secret is never shown, only whether one is saved.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { OidcPendingProvider } from '$lib/state/oidc.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    pending: OidcPendingProvider;
    busy: boolean;
    onTry: () => void;
    onDiscard: () => void;
  }
  let { pending, busy, onTry, onDiscard }: Props = $props();
  const uid = $props.id();
</script>

<section
  class="grid min-w-0 gap-2 rounded-md border border-dashed p-4"
  aria-labelledby="{uid}-title"
  data-testid="sso-pending"
>
  <h3 id="{uid}-title" class="text-sm font-semibold">{m.org_sso_pending_title()}</h3>
  <p class="text-xs text-muted-foreground">{m.org_sso_pending_info()}</p>
  <dl class="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm">
    <dt class="text-muted-foreground">{m.org_sso_issuer()}</dt>
    <dd class="break-all" data-testid="sso-pending-issuer">{pending.issuer}</dd>
    <dt class="text-muted-foreground">{m.org_sso_client()}</dt>
    <dd class="break-all" data-testid="sso-pending-client">{pending.clientId}</dd>
    <dt class="text-muted-foreground">{m.org_sso_secret()}</dt>
    <dd>{pending.hasClientSecret ? m.org_sso_secret_stored() : m.org_sso_secret_none()}</dd>
  </dl>
  <div class="flex flex-wrap gap-2">
    <Button size="sm" disabled={busy} data-testid="sso-pending-try" onclick={onTry}
      >{m.org_sso_pending_try()}</Button
    >
    <Button
      size="sm"
      variant="outline"
      disabled={busy}
      data-testid="sso-pending-discard"
      onclick={onDiscard}>{m.org_sso_pending_discard()}</Button
    >
  </div>
</section>
