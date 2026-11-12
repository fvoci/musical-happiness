<!--
  The sign-ins connected to the reader's account, in the settings: who they
  are at the single sign-on of their organizations, by which that sign-on
  signs in to this account. Connecting goes through an organization's provider
  and comes back; both connecting and disconnecting ask for the account's
  password, as changing the password does. A refusal is told by its reason
  (`linked-sign-ins.ts`).

  The account is one for every organization: a sign-in connected through the
  provider of one signs in to the whole account. The form says whose it is
  before the reader goes.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { LinkedIdentity, SignInOrganization } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  import { issuerHost, linkProblemOf } from './linked-sign-ins.ts';

  interface Props {
    /** The identities connected to the account, oldest first. */
    read: () => Promise<LinkedIdentity[]>;
    /** The reader's organizations that sign in through a provider. */
    organizations: () => Promise<SignInOrganization[]>;
    /** Starts connecting through the organization's provider and sends the browser there. */
    onLink: (orgId: string, password: string) => Promise<void>;
    onUnlink: (identity: LinkedIdentity, password: string) => Promise<void>;
    /** A sign-in was connected just now: said once. */
    justLinked?: boolean;
  }

  let { read, organizations, onLink, onUnlink, justLinked = false }: Props = $props();

  const uid = $props.id();
  let identities = $state.raw<LinkedIdentity[] | undefined>(undefined);
  let offered = $state.raw<SignInOrganization[]>([]);
  /** What is open: the form that connects, or the identity being disconnected. */
  let open = $state.raw<'connect' | LinkedIdentity | undefined>(undefined);
  let orgId = $state('');
  let password = $state('');
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  let removed = $state(false);
  let passwordField = $state<HTMLInputElement | null>(null);

  const keyOf = (identity: LinkedIdentity): string => `${identity.issuer}\n${identity.subject}`;

  async function load(): Promise<void> {
    try {
      [identities, offered] = await Promise.all([read(), organizations()]);
      orgId = offered[0]?.orgId ?? '';
    } catch {
      identities = [];
    }
  }
  onMount(() => {
    void load();
  });

  function show(next: 'connect' | LinkedIdentity | undefined): void {
    open = next;
    password = '';
    problem = undefined;
    removed = false;
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const doing = open;
    if (working || doing === undefined) return;
    working = true;
    problem = undefined;
    try {
      if (doing === 'connect') {
        // The browser goes to the provider: the page ends here.
        await onLink(orgId, password);
        return;
      }
      await onUnlink(doing, password);
      show(undefined);
      removed = true;
      identities = await read();
    } catch (error) {
      const told = linkProblemOf(error);
      problem = told.text;
      if (told.password) passwordField?.focus();
    } finally {
      working = false;
    }
  }
</script>

{#snippet passwordForm(action: string)}
  <form class="grid gap-3" onsubmit={(event) => void submit(event)}>
    <div class="grid gap-1.5">
      <Label for="{uid}-password">{m.account_password_current()}</Label>
      <Input
        bind:ref={passwordField}
        id="{uid}-password"
        type="password"
        bind:value={password}
        autocomplete="current-password"
        required
        aria-invalid={problem ? 'true' : undefined}
        aria-describedby={problem ? `${uid}-problem` : undefined}
        class="roomy:h-touch roomy:text-base"
      />
    </div>
    {#if problem}
      <p id="{uid}-problem" class="text-sm text-destructive" role="alert">{problem}</p>
    {/if}
    <div class="flex flex-wrap gap-2">
      <Button type="submit" class="roomy:min-h-touch" disabled={working}>
        {working ? m.account_working() : action}
      </Button>
      <Button
        type="button"
        variant="outline"
        class="roomy:min-h-touch"
        onclick={() => {
          show(undefined);
        }}>{m.shared_cancel()}</Button
      >
    </div>
  </form>
{/snippet}

<section
  class="grid gap-3 border-t pt-4"
  aria-labelledby="{uid}-title"
  data-testid="linked-sign-ins"
>
  <div class="grid gap-0.5">
    <h3 id="{uid}-title" class="font-medium">{m.account_linked_title()}</h3>
    <p class="text-xs text-muted-foreground">{m.account_linked_hint()}</p>
  </div>

  <!-- In the page from the start: a live region that appears with its words is often not read. -->
  <p class="text-sm text-muted-foreground empty:hidden" role="status">
    {#if removed}{m.account_linked_removed()}{:else if justLinked}{m.account_linked_done()}{/if}
  </p>

  {#if identities === undefined}
    <p class="text-sm text-muted-foreground">{m.app_loading()}</p>
  {:else if identities.length === 0}
    <p class="text-sm text-muted-foreground">{m.account_linked_none()}</p>
  {:else}
    <ul class="grid gap-2">
      {#each identities as identity (keyOf(identity))}
        <li class="grid gap-2" data-testid="linked-sign-in">
          <div class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
            <span class="grid min-w-0 flex-1">
              <span class="truncate" translate="no">{issuerHost(identity.issuer)}</span>
              {#if identity.email}
                <span class="truncate text-xs text-muted-foreground" translate="no">
                  {identity.email}
                </span>
              {/if}
            </span>
            {#if open !== identity}
              <Button
                variant="outline"
                class="roomy:min-h-touch"
                onclick={() => {
                  show(identity);
                }}>{m.account_linked_disconnect()}</Button
              >
            {/if}
          </div>
          {#if open === identity}
            {@render passwordForm(m.account_linked_disconnect())}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if identities !== undefined}
    {#if offered.length === 0}
      <p class="text-xs text-muted-foreground">{m.account_linked_none_offered()}</p>
    {:else if open === 'connect'}
      <div class="grid gap-3" data-testid="linked-connect-form">
        <div class="grid gap-1.5">
          <Label for="{uid}-which">{m.account_linked_which()}</Label>
          <select
            id="{uid}-which"
            bind:value={orgId}
            class="h-9 rounded-md border bg-background px-3 text-sm roomy:h-touch roomy:text-base"
          >
            {#each offered as one (one.orgId)}
              <option value={one.orgId}>{one.name}</option>
            {/each}
          </select>
          <p class="text-xs text-muted-foreground">{m.account_linked_whole_account()}</p>
        </div>
        {@render passwordForm(m.account_linked_continue())}
      </div>
    {:else}
      <Button
        variant="outline"
        class="justify-self-start roomy:min-h-touch"
        data-testid="linked-connect"
        onclick={() => {
          show('connect');
        }}>{m.account_linked_connect()}</Button
      >
    {/if}
  {/if}
</section>
