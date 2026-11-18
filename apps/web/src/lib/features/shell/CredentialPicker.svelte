<!--
  The reader's degrees and qualifications, in the settings: up to three from
  the list, written after the name in the order they were added. Each listed
  one has its button to remove it; the list to add from leaves out those
  listed, and is off once three are.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { CREDENTIALS, MAX_CREDENTIALS, type Credential } from '$lib/logic/person-names.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { CREDENTIAL_LABELS } from './name-labels.ts';

  let {
    value = $bindable(),
    disabled = false,
    problem,
    onchange,
  }: {
    value: Credential[];
    disabled?: boolean;
    problem?: string;
    onchange?: () => void;
  } = $props();

  const uid = $props.id();
  const full = $derived(value.length >= MAX_CREDENTIALS);
  const left = $derived(CREDENTIALS.filter((code) => !value.includes(code)));

  function add(code: string): void {
    const known = left.find((credential) => credential === code);
    if (!known || full) return;
    value = [...value, known];
    onchange?.();
  }

  function remove(code: Credential): void {
    value = value.filter((listed) => listed !== code);
    onchange?.();
  }
</script>

<fieldset class="grid gap-1.5" aria-describedby="{uid}-hint">
  <legend class="mb-1 font-medium">{m.account_name_credentials()}</legend>
  {#if value.length > 0}
    <ol class="flex flex-wrap gap-2" data-testid="credentials">
      {#each value as code (code)}
        <li
          class="flex items-center gap-1 rounded-md border border-input-border py-0.5 ps-2"
          data-credential={code}
        >
          <span>{CREDENTIAL_LABELS[code]()}</span>
          <Button
            variant="ghost"
            size="sm"
            class="h-7 px-2 roomy:min-h-touch"
            {disabled}
            aria-label={m.account_name_credential_remove({ credential: CREDENTIAL_LABELS[code]() })}
            onclick={() => {
              remove(code);
            }}
          >
            <span aria-hidden="true">×</span>
          </Button>
        </li>
      {/each}
    </ol>
  {/if}
  <select
    id="{uid}-add"
    class="h-8 max-w-full rounded-md border border-input-border bg-background px-2 roomy:h-touch"
    aria-label={m.account_name_credentials_add()}
    aria-invalid={problem ? 'true' : undefined}
    disabled={disabled || full}
    value=""
    data-testid="credential-add"
    onchange={(event) => {
      add(event.currentTarget.value);
      event.currentTarget.value = '';
    }}
  >
    <option value="" disabled>{m.account_name_credentials_add()}</option>
    {#each left as code (code)}
      <option value={code}>{CREDENTIAL_LABELS[code]()}</option>
    {/each}
  </select>
  {#if problem}
    <p class="text-sm text-destructive" role="alert" data-testid="problem-credentials">
      {problem}
    </p>
  {/if}
  <p id="{uid}-hint" class="text-xs text-muted-foreground" aria-live="polite">
    {full ? m.account_name_credentials_full() : m.account_name_credentials_hint()}
  </p>
</fieldset>
