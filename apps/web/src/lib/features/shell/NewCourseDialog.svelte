<!--
  "New course": its name, and who finds it in a shared workspace (open to
  anyone in the workspace to read and join, or invited only). In the reader's
  own space (`alone`), visibility is not asked and the course is private.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { ContainerType } from '$lib/logic/wire.ts';
  import type { CourseDraft } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Input } from '$lib/ui/input/index.js';

  interface Props {
    open: boolean;
    workspaceName: string;
    /** Nobody else works in the workspace (the reader's own space). */
    alone?: boolean;
    onCreate: (draft: CourseDraft) => void;
  }

  let { open = $bindable(), workspaceName, alone = false, onCreate }: Props = $props();

  const uid = $props.id();
  let title = $state('');
  let type = $state<ContainerType>('O');

  function opened(): void {
    title = '';
    type = 'O';
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    open = false;
    onCreate({ title: title.trim(), type: alone ? 'P' : type });
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content data-testid="new-course-dialog" {@attach opened}>
    <form class="grid gap-4" onsubmit={submit}>
      <Dialog.Header>
        <Dialog.Title>{m.course_new_title()}</Dialog.Title>
        <Dialog.Description>{m.course_new_description()}</Dialog.Description>
      </Dialog.Header>
      <Input
        bind:value={title}
        aria-label={m.course_name()}
        placeholder={m.course_name()}
        maxlength={255}
        autocomplete="off"
        enterkeyhint="done"
        data-testid="new-course-title"
        class="roomy:h-touch roomy:text-base"
      />
      {#if !alone}
        <fieldset class="grid gap-2">
          <legend class="mb-1 font-medium">{m.course_new_who()}</legend>
          <label class="flex min-h-8 items-start gap-3 roomy:min-h-touch" for="{uid}-open">
            <input
              id="{uid}-open"
              type="radio"
              name="{uid}-type"
              value="O"
              bind:group={type}
              data-testid="new-course-open"
              class="mt-1 size-4 accent-primary"
            />
            <span>{m.course_new_open({ workspace: workspaceName })}</span>
          </label>
          <label class="flex min-h-8 items-start gap-3 roomy:min-h-touch" for="{uid}-private">
            <input
              id="{uid}-private"
              type="radio"
              name="{uid}-type"
              value="P"
              bind:group={type}
              data-testid="new-course-private"
              class="mt-1 size-4 accent-primary"
            />
            <span>{m.course_new_private()}</span>
          </label>
        </fieldset>
      {/if}
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
        <Button type="submit" class="roomy:min-h-touch" data-testid="new-course-create">
          {m['Categories.CreateCategoryDialog.CreateText']()}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
