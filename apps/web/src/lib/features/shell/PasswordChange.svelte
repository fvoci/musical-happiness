<!--
  Changing the reader's password, in the settings. Folded until asked for:
  the current password and the new one. The server checks both (the length
  too: it counts characters, a browser's `minlength` counts UTF-16 units);
  a refusal is told by its reason, never by the server's own words. Once it
  is changed, the reader's other sessions are over, and this one goes on.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  interface Props {
    onChange: (oldPassword: string, newPassword: string) => Promise<void>;
  }

  let { onChange }: Props = $props();

  const uid = $props.id();
  let open = $state(false);
  let current = $state('');
  let next = $state('');
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  let done = $state(false);

  function problemOf(error: unknown): string {
    if (!isBackendError(error)) return m.account_failed();
    if (error.reason === 'invalid_credentials') return m.account_password_wrong();
    if (error.status === 429) return m.account_rate_limited();
    if (error.status === 400 && error.field === 'newPassword') {
      return m.account_password_too_short();
    }
    return m.account_failed();
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (working) return;
    working = true;
    problem = undefined;
    done = false;
    try {
      await onChange(current, next);
      current = '';
      next = '';
      open = false;
      done = true;
    } catch (error) {
      problem = problemOf(error);
    } finally {
      working = false;
    }
  }
</script>

<div class="grid gap-3 border-t pt-4" data-testid="password-change">
  <div class="flex min-h-8 items-center gap-3 roomy:min-h-touch">
    <span class="min-w-0 flex-1 font-medium">{m.account_password_change()}</span>
    <Button
      variant="outline"
      class="roomy:min-h-touch"
      aria-expanded={open}
      aria-controls={open ? `${uid}-form` : undefined}
      onclick={() => {
        open = !open;
        problem = undefined;
        done = false;
      }}
    >
      {open ? m.shared_cancel() : m.account_password_change()}
    </Button>
  </div>

  {#if done}
    <p class="text-sm text-muted-foreground" role="status">{m.account_password_changed()}</p>
  {/if}

  {#if open}
    <form id="{uid}-form" class="grid gap-3" onsubmit={(event) => void submit(event)}>
      <div class="grid gap-1.5">
        <Label for="{uid}-current">{m.account_password_current()}</Label>
        <Input
          id="{uid}-current"
          type="password"
          bind:value={current}
          autocomplete="current-password"
          required
          class="roomy:h-touch roomy:text-base"
        />
      </div>
      <div class="grid gap-1.5">
        <Label for="{uid}-new">{m.account_password_new()}</Label>
        <Input
          id="{uid}-new"
          type="password"
          bind:value={next}
          autocomplete="new-password"
          required
          aria-describedby="{uid}-hint"
          class="roomy:h-touch roomy:text-base"
        />
        <p id="{uid}-hint" class="text-xs text-muted-foreground">{m.account_password_hint()}</p>
      </div>
      {#if problem}
        <p class="text-sm text-destructive" role="alert">{problem}</p>
      {/if}
      <Button type="submit" class="justify-self-start roomy:min-h-touch" disabled={working}>
        {working ? m.account_working() : m.account_password_save()}
      </Button>
    </form>
  {/if}
</div>
