<!--
  "New wiki": its name, and who finds it, as a board's sharing says it: open
  to whoever works in the workspace to find and join, or for the invited only.
  The field holds the focus; Enter makes it. A wiki without a name is made too,
  and shown under the words for one without.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { ContainerType } from '$lib/logic/wire.ts';
  import type { WikiDraft } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Input } from '$lib/ui/input/index.js';

  interface Props {
    open: boolean;
    /** The workspace's name, as the sharing of a board says it. */
    workspaceName: string;
    onCreate: (draft: WikiDraft) => void;
  }

  let { open = $bindable(), workspaceName, onCreate }: Props = $props();

  const uid = $props.id();
  let title = $state('');
  let type = $state<ContainerType>('O');

  /** Each opening starts afresh. */
  function opened(): void {
    title = '';
    type = 'O';
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    open = false;
    onCreate({ title: title.trim(), type });
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content data-testid="new-wiki-dialog" {@attach opened}>
    <form class="grid gap-4" onsubmit={submit}>
      <Dialog.Header>
        <Dialog.Title>{m.wiki_new_title()}</Dialog.Title>
        <Dialog.Description>{m.wiki_new_description()}</Dialog.Description>
      </Dialog.Header>
      <Input
        bind:value={title}
        aria-label={m.wiki_name()}
        placeholder={m.wiki_name()}
        maxlength={255}
        autocomplete="off"
        enterkeyhint="done"
        data-testid="new-wiki-title"
        class="roomy:h-touch roomy:text-base"
      />
      <fieldset class="grid gap-2">
        <legend class="mb-1 font-medium">{m.wiki_new_who()}</legend>
        <label class="flex min-h-8 items-start gap-3 roomy:min-h-touch" for="{uid}-open">
          <input
            id="{uid}-open"
            type="radio"
            name="{uid}-type"
            value="O"
            bind:group={type}
            data-testid="new-wiki-open"
            class="mt-1 size-4 accent-primary"
          />
          <span>{m.wiki_new_open({ workspace: workspaceName })}</span>
        </label>
        <label class="flex min-h-8 items-start gap-3 roomy:min-h-touch" for="{uid}-private">
          <input
            id="{uid}-private"
            type="radio"
            name="{uid}-type"
            value="P"
            bind:group={type}
            data-testid="new-wiki-private"
            class="mt-1 size-4 accent-primary"
          />
          <span>{m.wiki_new_private()}</span>
        </label>
      </fieldset>
      <Dialog.Footer>
        <Button
          variant="outline"
          class="roomy:min-h-touch"
          onclick={() => {
            open = false;
          }}
        >
          {m.shared_cancel()}
        </Button>
        <Button type="submit" class="roomy:min-h-touch" data-testid="new-wiki-create">
          {m['Categories.CreateCategoryDialog.CreateText']()}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
