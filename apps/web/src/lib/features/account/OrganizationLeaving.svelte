<!--
  Leaving an organization, in the settings: each organization the reader is a
  member of, with a way out of it. Asked, a dialog says what leaving comes
  to before anything changes: out of its workspaces, boards and wikis at
  once, what they wrote stays, and coming back takes an invitation. An
  account without a password is told first to set one, since it signs in
  through an organization's provider only.

  A refusal (the only administrator of the organization or of the server,
  the only person in it) is said in the dialog with what to do first, and
  the dialog stays; nothing changed. Left, the organization goes from the
  list, and the page says so.
-->
<script lang="ts">
  import { tick } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import * as AlertDialog from '$lib/ui/alert-dialog/index.js';
  import { Button } from '$lib/ui/button/index.js';

  import {
    leaveProblemOf,
    type LeavableOrganization,
    type LeaveProblem,
  } from './organization-leaving.ts';

  interface Props {
    /** The organizations the reader can leave (`leavableOrganizations`). */
    organizations: readonly LeavableOrganization[];
    /** Whether the account has a password; one without signs in through a provider only. */
    hasPassword: boolean;
    /** Leaves the organization; a refusal rejects. */
    onLeave: (orgId: string) => Promise<void>;
  }

  let { organizations, hasPassword, onLeave }: Props = $props();

  let open = $state(false);
  /** The organization the dialog asks about; kept while it closes. */
  let asked = $state.raw<LeavableOrganization | undefined>(undefined);
  let working = $state(false);
  let problem = $state.raw<LeaveProblem | undefined>(undefined);
  /** What was left last, said to the reader once the dialog is gone. */
  let left = $state('');
  let heading = $state<HTMLElement | null>(null);
  /** The button that asked: the focus comes back to it, if it is still there. */
  let asker: HTMLElement | undefined;

  function ask(organization: LeavableOrganization, event: MouseEvent): void {
    asker = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined;
    problem = undefined;
    left = '';
    asked = organization;
    open = true;
  }

  function close(): void {
    if (!working) open = false;
  }

  async function leave(): Promise<void> {
    const leaving = asked;
    if (!leaving || working) return;
    working = true;
    problem = undefined;
    try {
      await onLeave(leaving.orgId);
      left = m.org_leave_left({ name: leaving.name });
      working = false;
      open = false;
    } catch (error) {
      problem = leaveProblemOf(error);
      working = false;
    }
  }
</script>

{#if organizations.length > 0 || left}
  <div class="grid gap-3 border-t pt-4" data-testid="organization-leaving">
    <p bind:this={heading} tabindex="-1" class="font-medium outline-none">
      {m.org_leave_title()}
    </p>
    {#if organizations.length > 0}
      <ul class="grid gap-2">
        {#each organizations as organization (organization.orgId)}
          <li class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
            <span class="min-w-0 flex-1 truncate" title={organization.name}
              >{organization.name}</span
            >
            <Button
              variant="outline"
              class="roomy:min-h-touch"
              aria-label={m.org_leave_button_label({ name: organization.name })}
              onclick={(event: MouseEvent) => {
                ask(organization, event);
              }}
            >
              {m.org_leave_button()}
            </Button>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="text-sm text-muted-foreground empty:hidden" role="status">{left}</p>
  </div>
{/if}

<AlertDialog.Root
  bind:open={
    () => open,
    (next: boolean) => {
      if (!next) close();
    }
  }
>
  <AlertDialog.Content
    data-testid="organization-leave-dialog"
    onCloseAutoFocus={(event: Event) => {
      // Left, its button is gone: the focus stays in the section, at its heading.
      const target = asker?.isConnected ? asker : heading;
      if (!target) return;
      event.preventDefault();
      void tick().then(() => {
        target.focus();
      });
    }}
  >
    {#if asked}
      <AlertDialog.Header>
        <AlertDialog.Title>{m.org_leave_confirm_title({ name: asked.name })}</AlertDialog.Title>
        <AlertDialog.Description>{m.org_leave_confirm_text()}</AlertDialog.Description>
      </AlertDialog.Header>
      {#if !hasPassword}
        <p
          class="text-sm font-medium text-destructive"
          data-testid="organization-leave-password-first"
        >
          {m.org_leave_password_first()}
        </p>
      {/if}
      <ul class="grid list-disc gap-1 ps-5 text-sm">
        <li>{m.org_leave_what_goes()}</li>
        <li>{m.org_leave_what_stays()}</li>
        <li>{m.org_leave_coming_back()}</li>
      </ul>
      {#if problem}
        <p class="text-sm text-destructive" role="alert" data-testid="organization-leave-problem">
          {problem.text}
        </p>
      {/if}
      <AlertDialog.Footer>
        <AlertDialog.Cancel class="roomy:min-h-touch" disabled={working}>
          {m.shared_cancel()}
        </AlertDialog.Cancel>
        <Button
          variant="destructive"
          class="roomy:min-h-touch"
          disabled={working || problem?.settled === true}
          onclick={() => void leave()}
        >
          {working ? m.account_working() : m.org_leave_button()}
        </Button>
      </AlertDialog.Footer>
    {/if}
  </AlertDialog.Content>
</AlertDialog.Root>
