<!--
  The description of a board or a wiki, behind "ⓘ" next to its title: the
  title row keeps its room for the title, and the description is read where
  it opens (a popover; a sheet on a compact page). There is no button where
  there is no description. A narrow pane (a phone) has no room for the button
  beside the title: the top bar opens it from its menu (`open`).

  A reader who may change it (`onChange`) changes it there, in a field: what
  is typed is handed on, once, when the surface closes; Escape in the field
  puts the description back first. While an input method is composing
  (Hangul, kana) Escape is its own.
-->
<script lang="ts">
  import InfoIcon from '@lucide/svelte/icons/info';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PickerSurface from '$lib/components/picker-surface/PickerSurface.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { isComposing } from '$lib/keys.ts';
  import { Textarea } from '$lib/ui/textarea/index.js';

  interface Props {
    /** Bindable: the top bar's menu opens it where the button is away. */
    open?: boolean;
    description: string;
    /** Without it the description is only read. */
    onChange?: (description: string) => void;
  }

  let { open = $bindable(false), description, onChange }: Props = $props();

  const presentation = new PickerPresentation();
  /** What the reader has typed and not yet handed on. */
  let draft = $state<string | undefined>(undefined);

  function commit(): void {
    if (draft === undefined) return;
    const next = draft.trim();
    draft = undefined;
    if (next !== description) onChange?.(next);
  }

  function onkeydown(event: KeyboardEvent): void {
    // Escape goes on to the surface, which closes; nothing typed is kept.
    if (event.key === 'Escape' && !isComposing(event)) draft = undefined;
  }
</script>

<PickerSurface
  bind:open={
    () => open,
    (next: boolean) => {
      open = next;
      if (!next) commit();
    }
  }
  presentation={presentation.current}
  title={m.shell_description()}
>
  {#snippet trigger({ props }: { props: Record<string, unknown> })}
    <IconButton
      {...props}
      label={m.shell_description()}
      data-testid="container-description"
      class="shrink-0 text-muted-foreground @max-columns/pane:hidden"
    >
      <InfoIcon />
    </IconButton>
  {/snippet}
  <div
    class="w-80 max-w-full p-1 group-data-[presentation=sheet]/surface:w-full"
    data-testid="container-description-panel"
  >
    {#if onChange}
      <Textarea
        value={draft ?? description}
        aria-label={m.shell_description()}
        maxlength={2000}
        rows={3}
        data-picker-focus
        data-testid="container-description-field"
        class="max-h-80 roomy:text-base"
        oninput={(event: Event & { currentTarget: HTMLTextAreaElement }) => {
          draft = event.currentTarget.value;
        }}
        {onkeydown}
      />
    {:else}
      <p
        class="max-h-80 overflow-y-auto text-sm break-words whitespace-pre-line"
        data-testid="container-description-text"
      >
        {description}
      </p>
    {/if}
  </div>
</PickerSurface>
