<!--
  The personal API tokens of the organization's members that reach its
  workspaces, on the organization screen: whose each is, what it may do,
  where, and until when. An administrator ends one at once, for every
  organization it reaches, after being asked.
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { ApiToken } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    read: () => Promise<ApiToken[]>;
    revoke: (tokenId: string) => Promise<void>;
    /** The name a member is shown by. */
    nameOf: (userId: string) => string;
  }

  let { read, revoke, nameOf }: Props = $props();

  let tokens = $state.raw<ApiToken[] | undefined>(undefined);
  let unread = $state(false);
  let problem = $state('');
  let ending = $state.raw<ApiToken | undefined>(undefined);
  let confirmOpen = $state(false);
  let heading: HTMLElement | null = null;

  const dates = $derived(new Intl.DateTimeFormat(getLocale(), { dateStyle: 'medium' }));
  const scopeOf = (token: ApiToken): string =>
    token.scope === 'read' ? m.account_api_tokens_read() : m.account_api_tokens_write();

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

  async function end(token: ApiToken): Promise<void> {
    problem = '';
    try {
      await revoke(token.id);
    } catch {
      problem = m.org_refused_failed();
    }
    await load();
  }
</script>

<section class="flex flex-col gap-3" aria-labelledby="org-api-tokens" data-testid="org-api-tokens">
  <h2
    id="org-api-tokens"
    tabindex="-1"
    class="text-base font-semibold outline-none"
    {@attach (node: HTMLElement) => {
      heading = node;
      return () => {
        heading = null;
      };
    }}
  >
    {m.org_api_tokens_list()}
  </h2>
  <p class="text-sm text-destructive empty:hidden" role="alert">{problem}</p>
  {#if unread}
    <div class="flex items-center gap-3">
      <p class="min-w-0 flex-1 text-sm" role="alert">{m.app_problem_load()}</p>
      <Button variant="outline" class="roomy:min-h-touch" onclick={() => void load()}>
        {m.app_retry()}
      </Button>
    </div>
  {:else if tokens === undefined}
    <p class="text-sm text-muted-foreground">{m.app_loading()}</p>
  {:else if tokens.length === 0}
    <p class="text-sm text-muted-foreground">{m.org_api_tokens_none()}</p>
  {:else}
    <ul class="flex flex-col gap-2">
      {#each tokens as token (token.id)}
        <li
          class="flex flex-wrap items-center gap-2 rounded-md border px-3 py-2"
          data-testid="org-api-token"
        >
          <span class="grid min-w-0 flex-1">
            <span class="truncate">{token.name}</span>
            <span class="truncate text-xs text-muted-foreground">
              {m.org_api_tokens_owner({ name: nameOf(token.userId) })} · {scopeOf(token)} ·
              {token.workspaces.map((one) => one.name).join(', ')}
            </span>
            <span class="truncate text-xs text-muted-foreground">
              <span translate="no">{token.tokenPrefix}…</span> ·
              {m.account_api_tokens_expires({ date: dates.format(token.expiresAt) })}
            </span>
          </span>
          <Button
            size="sm"
            variant="outline"
            aria-label="{m.account_api_tokens_revoke()}: {token.name}"
            onclick={() => {
              ending = token;
              confirmOpen = true;
            }}
          >
            {m.account_api_tokens_revoke()}
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<ConfirmDialog
  bind:open={confirmOpen}
  title={m.org_api_tokens_revoke_title({ name: ending?.name ?? '' })}
  description={m.org_api_tokens_revoke_info()}
  confirmLabel={m.account_api_tokens_revoke()}
  destructive
  focusAfter={() => heading}
  onConfirm={() => {
    if (ending) void end(ending);
  }}
/>
