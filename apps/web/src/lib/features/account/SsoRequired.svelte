<!--
  What a workspace of an organization that takes no password shows to
  somebody signed in with one: that it is opened by signing in through the
  organization's single sign-on, and the button that goes there. The session
  stays as it is: the reader's other workspaces are a click away.

  Whoever has not connected single sign-on to their account yet is told
  where that is done: the provider would otherwise refuse them for their
  address being an account's already.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    /** The workspace's name; empty if it has none. */
    name: string;
    /** Starts the sign-in and sends the browser to the provider; a refusal rejects. */
    onContinue: () => Promise<void>;
    /** Opens the connected sign-ins of the settings. */
    onOpenLinked: () => void;
  }

  let { name, onContinue, onOpenLinked }: Props = $props();

  let working = $state(false);
  let failed = $state(false);

  async function go(): Promise<void> {
    if (working) return;
    working = true;
    failed = false;
    try {
      await onContinue();
    } catch {
      failed = true;
      working = false;
    }
  }
</script>

<div class="grid h-full place-items-center p-6 text-center" data-testid="sso-required">
  <div class="grid max-w-md justify-items-center gap-3">
    <h2 class="text-lg font-medium">
      {name ? m.account_sso_required_title_named({ name }) : m.account_sso_required_title()}
    </h2>
    <p class="text-muted-foreground">{m.account_sso_required_text()}</p>
    <Button
      class="roomy:min-h-touch"
      disabled={working}
      data-testid="sso-continue"
      onclick={() => void go()}
    >
      {working ? m.account_working() : m.account_sso_continue()}
    </Button>
    <!-- In the page from the start: a live region that appears with its words is often not read. -->
    <p class="text-sm text-destructive empty:hidden" role="alert">
      {failed ? m.account_oidc_failed() : ''}
    </p>
    <p class="text-sm text-muted-foreground">
      {m.account_sso_first_time()}
      <button
        type="button"
        class="underline underline-offset-4 roomy:py-3"
        data-testid="sso-open-linked"
        onclick={onOpenLinked}>{m.account_linked_open()}</button
      >
    </p>
  </div>
</div>
