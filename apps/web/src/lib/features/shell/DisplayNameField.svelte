<!--
  The reader's display name, in the settings: the name others see beside
  their login name ("Display name (@login)"). Saved with its button; empty is
  none. A name the server refuses is told beside the field, by the reason it
  gives (`errors` at `/displayName`); any other failure is the page's problem
  line, and the field keeps what was typed.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isBackendError, type Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Input } from '$lib/ui/input/index.js';
  import { Label } from '$lib/ui/label/index.js';

  let { workspace }: { workspace: Workspace } = $props();

  const uid = $props.id();
  const stored = $derived(workspace.me?.displayName ?? '');
  // svelte-ignore state_referenced_locally (the field starts from the stored name, then is the reader's)
  let value = $state(stored);
  let working = $state(false);
  let problem = $state<string | undefined>(undefined);
  let saved = $state(false);

  function problemOf(reason: string): string {
    if (reason === 'too_long') return m.account_display_name_too_long();
    if (reason === 'invalid_characters') return m.account_display_name_invalid_characters();
    return m.account_field_invalid();
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (working) return;
    working = true;
    problem = undefined;
    saved = false;
    try {
      await workspace.changeDisplayName(value);
      // As the server keeps it: without the white space around it.
      value = workspace.me?.displayName ?? '';
      saved = true;
    } catch (error) {
      const refused = isBackendError(error) ? error.refusalAt('/displayName') : undefined;
      if (refused) problem = problemOf(refused.reason);
      else workspace.report('save', error);
    } finally {
      working = false;
    }
  }
</script>

<form class="grid gap-1.5" data-setting="displayName" onsubmit={(event) => void save(event)}>
  <Label for="{uid}-name" class="font-medium">{m.account_display_name()}</Label>
  <div class="flex gap-2">
    <Input
      id="{uid}-name"
      name="displayName"
      bind:value
      autocomplete="name"
      oninput={() => {
        saved = false;
        problem = undefined;
      }}
      aria-invalid={problem ? 'true' : undefined}
      aria-describedby={problem ? `${uid}-problem ${uid}-hint` : `${uid}-hint`}
      class="min-w-0 flex-1 roomy:h-touch roomy:text-base"
    />
    <Button
      type="submit"
      variant="outline"
      class="roomy:min-h-touch"
      disabled={working || value === stored}
    >
      {m.account_display_name_save()}
    </Button>
  </div>
  {#if problem}
    <p
      id="{uid}-problem"
      class="text-sm text-destructive"
      role="alert"
      data-testid="problem-displayName"
    >
      {problem}
    </p>
  {/if}
  <p id="{uid}-hint" class="text-xs text-muted-foreground" aria-live="polite">
    {saved ? m.account_display_name_saved() : m.account_display_name_hint()}
  </p>
</form>
