<!--
  The reader's personal API tokens, in the settings: the keys they give an
  agent to work as them. "New token" unfolds the form (`ApiTokenForm.svelte`)
  under it; a token made is shown once, as the command that connects Claude
  Code with it, copied by one press, with a line on the shell's history. The
  list says what each may do, where, until when and when it was last used,
  and ends one at once.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { ApiToken, IssuedApiToken, NewApiToken } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import ApiTokenForm from './ApiTokenForm.svelte';
  import {
    lastUsedOf,
    mcpCommand,
    scopeLabel,
    tokenProblemOf,
    type TokenWorkspace,
  } from './api-tokens.ts';

  interface Props {
    /** The reader's tokens, the newest first. */
    read: () => Promise<ApiToken[]>;
    issue: (token: NewApiToken) => Promise<IssuedApiToken>;
    revoke: (tokenId: string) => Promise<void>;
    workspaces: readonly TokenWorkspace[];
    currentWorkspaceId: string | undefined;
    hasOwnSpace: boolean;
    hasPassword: boolean;
    /** The server's origin, which the command names. */
    origin: string;
  }

  let {
    read,
    issue,
    revoke,
    workspaces,
    currentWorkspaceId,
    hasOwnSpace,
    hasPassword,
    origin,
  }: Props = $props();

  const uid = $props.id();
  let tokens = $state.raw<ApiToken[] | undefined>(undefined);
  let unread = $state(false);
  let open = $state(false);
  let issued = $state.raw<IssuedApiToken | undefined>(undefined);
  let copied = $state(false);
  let status = $state('');
  let problem = $state('');
  let toggle = $state<HTMLElement | null>(null);

  const command = $derived(issued ? mcpCommand(origin, issued.token) : '');
  const dates = $derived(new Intl.DateTimeFormat(getLocale(), { dateStyle: 'medium' }));

  async function load(): Promise<void> {
    unread = false;
    try {
      tokens = await read();
    } catch {
      tokens = undefined;
      unread = true;
    }
  }
  onMount(() => {
    void load();
  });

  function made(token: IssuedApiToken): void {
    issued = token;
    open = false;
    copied = false;
    status = m.account_api_tokens_made();
    tokens = [token.apiToken, ...(tokens ?? [])];
  }

  /** The token made is where the focus goes: the form it came from is gone. */
  function focusOnShow(node: HTMLElement): void {
    node.focus();
  }

  /** Copies the command; where the browser lets no page write the clipboard, says to copy it by hand. */
  async function copy(): Promise<void> {
    problem = '';
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
    } catch {
      problem = m.account_api_tokens_copy_failed();
    }
  }

  function done(): void {
    issued = undefined;
    status = '';
    toggle?.focus();
  }

  async function end(token: ApiToken): Promise<void> {
    problem = '';
    try {
      await revoke(token.id);
      tokens = (tokens ?? []).filter((one) => one.id !== token.id);
      if (issued?.apiToken.id === token.id) issued = undefined;
      status = m.account_api_tokens_revoked();
      toggle?.focus();
    } catch (error) {
      problem = tokenProblemOf(error).text;
    }
  }

  function where(token: ApiToken): string {
    const names = token.workspaces.map((one) => one.name);
    if (token.allowPersonalSpace) names.push(m.account_api_tokens_own_space_name());
    return names.join(', ');
  }
</script>

<section class="grid gap-3 border-t pt-4" aria-labelledby="{uid}-title" data-testid="api-tokens">
  <div class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
    <div class="grid min-w-0 flex-1 gap-0.5">
      <h3 id="{uid}-title" class="font-medium">{m.account_api_tokens_title()}</h3>
      <p class="text-xs text-muted-foreground">{m.account_api_tokens_hint()}</p>
    </div>
    <Button
      bind:ref={toggle}
      variant="outline"
      class="roomy:min-h-touch"
      aria-expanded={open}
      data-testid="api-token-new"
      onclick={() => {
        open = !open;
        issued = undefined;
        status = '';
      }}
    >
      {open ? m.shared_cancel() : m.account_api_tokens_new()}
    </Button>
  </div>

  <!-- In the page from the start: a live region that appears with its words is often not read. -->
  <p class="text-sm text-muted-foreground empty:hidden" role="status">{status}</p>
  <p class="text-sm text-destructive empty:hidden" role="alert">{problem}</p>

  {#if open}
    <ApiTokenForm
      {workspaces}
      {currentWorkspaceId}
      {hasOwnSpace}
      {hasPassword}
      {issue}
      onIssued={made}
    />
  {/if}

  {#if issued}
    <div
      {@attach focusOnShow}
      tabindex="-1"
      class="grid gap-2 rounded-md border p-3 outline-none"
      data-testid="api-token-issued"
    >
      <p class="text-sm font-medium">{m.account_api_tokens_command()}</p>
      <code
        class="block overflow-x-auto rounded bg-muted p-2 font-mono text-xs break-all whitespace-pre-wrap"
        translate="no"
        data-testid="api-token-command">{command}</code
      >
      <p class="text-xs text-muted-foreground">{m.account_api_tokens_history()}</p>
      <div class="flex gap-2">
        <Button class="roomy:min-h-touch" onclick={() => void copy()} data-testid="api-token-copy">
          {copied ? m.account_api_tokens_copied() : m.account_api_tokens_copy()}
        </Button>
        <Button variant="outline" class="roomy:min-h-touch" onclick={done}>
          {m.account_api_tokens_done()}
        </Button>
      </div>
    </div>
  {/if}

  {#if unread}
    <div class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
      <p class="min-w-0 flex-1 text-sm" role="alert">{m.app_problem_load()}</p>
      <Button variant="outline" class="roomy:min-h-touch" onclick={() => void load()}>
        {m.app_retry()}
      </Button>
    </div>
  {:else if tokens === undefined}
    <p class="text-sm text-muted-foreground">{m.app_loading()}</p>
  {:else if tokens.length === 0}
    <p class="text-sm text-muted-foreground">{m.account_api_tokens_none()}</p>
  {:else}
    <ul class="grid gap-2">
      {#each tokens as token (token.id)}
        <li class="flex min-h-8 items-center gap-3 roomy:min-h-touch" data-testid="api-token">
          <span class="grid min-w-0 flex-1">
            <span class="truncate">{token.name}</span>
            <span class="truncate text-xs text-muted-foreground">
              {scopeLabel(token.scope)} · {where(token)}
            </span>
            <span class="truncate text-xs text-muted-foreground">
              <span translate="no">{token.tokenPrefix}…</span> ·
              {m.account_api_tokens_expires({ date: dates.format(token.expiresAt) })} ·
              {lastUsedOf(token, dates)}
            </span>
          </span>
          <Button
            variant="outline"
            class="roomy:min-h-touch"
            aria-label="{m.account_api_tokens_revoke()}: {token.name}"
            onclick={() => void end(token)}
          >
            {m.account_api_tokens_revoke()}
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
