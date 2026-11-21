<!--
  What the reader's own space shows to a session made through a provider once
  the account has a password: that it opens only to a session made with the
  password, and the password asked for here, under the account's user name.
  Signed in with it (`onSignIn`), the page goes on in the own space; the
  session it replaces was good, and nothing else of the page closes.

  Shown in place of the space, often right after the password was set from the
  notice, whose form is gone with it: the field takes the focus when nothing
  else has it.
-->
<script lang="ts">
  import LockKeyholeIcon from '@lucide/svelte/icons/lock-keyhole';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  import { problemOf } from './problems.ts';

  interface Props {
    /** The user name of the account, which the password signs in. */
    username: string;
    /** Signs in with the password, and opens the own space; a refusal rejects. */
    onSignIn: (password: string) => Promise<void>;
  }

  let { username, onSignIn }: Props = $props();

  const uid = $props.id();
  let password = $state('');
  let working = $state(false);
  let problem = $state('');

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (working) return;
    working = true;
    problem = '';
    try {
      // Signed in, the page opens the own space: the form stays as it is meanwhile.
      await onSignIn(password);
    } catch (error) {
      problem = problemOf(error);
      working = false;
    }
  }

  /** Takes the focus when the screen comes in place of what had it. */
  function focusIfLost(field: HTMLElement): void {
    queueMicrotask(() => {
      const active = document.activeElement;
      if (active === null || active === document.body) field.focus();
    });
  }
</script>

<div class="grid h-full place-items-center p-6" data-testid="own-space-locked">
  <form
    class="grid w-full max-w-sm gap-3"
    aria-labelledby="{uid}-title"
    onsubmit={(event) => void submit(event)}
  >
    <div class="grid justify-items-center gap-2 text-center">
      <LockKeyholeIcon class="size-8 text-muted-foreground" aria-hidden="true" />
      <h2 id="{uid}-title" class="text-lg font-medium">{m.own_space_locked_title()}</h2>
      <p class="text-sm text-muted-foreground">{m.own_space_locked_text()}</p>
    </div>
    <p class="text-sm" data-testid="own-space-locked-user">
      {m.own_space_locked_as({ username })}
    </p>
    <Label for="{uid}-password">{m.account_password()}</Label>
    <Input
      {@attach focusIfLost}
      id="{uid}-password"
      type="password"
      bind:value={password}
      autocomplete="current-password"
      required
      aria-invalid={problem ? 'true' : undefined}
      aria-describedby="{uid}-problem"
      data-testid="own-space-locked-password"
      class="roomy:h-touch roomy:text-base"
    />
    <!-- In the page from the start: a live region that appears with its words is often not read. -->
    <p
      id="{uid}-problem"
      class="text-sm text-destructive empty:hidden"
      role="alert"
      data-testid="own-space-locked-problem"
    >
      {problem}
    </p>
    <Button type="submit" class="roomy:min-h-touch" disabled={working}>
      {working ? m.account_working() : m.own_space_locked_sign_in()}
    </Button>
  </form>
</div>
