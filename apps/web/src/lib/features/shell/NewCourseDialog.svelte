<!--
  "New course": start empty or from a course template, its name, and who finds
  it in a shared workspace (open to anyone in the workspace to read and join, or
  invited only). In the reader's own space (`alone`), visibility is not asked
  and the course is private.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { Container, ContainerType } from '$lib/logic/wire.ts';
  import type { CourseDraft, Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Input } from '$lib/ui/input/index.js';

  interface Props {
    open: boolean;
    workspaceName: string;
    /** Workspace to load available course templates from, when provided. */
    workspace?: Workspace | undefined;
    /** Nobody else works in the workspace (the reader's own space). */
    alone?: boolean;
    onCreate: (draft: CourseDraft, templateId?: string) => void;
  }

  let { open = $bindable(), workspaceName, workspace, alone = false, onCreate }: Props = $props();

  const uid = $props.id();
  let title = $state('');
  let type = $state<ContainerType>('O');
  let selectedTemplateId = $state('');
  let courseTemplates = $state.raw<Container[]>([]);
  let readings = 0;

  function opened(): void {
    untrack(() => {
      title = '';
      type = 'O';
      selectedTemplateId = '';
      courseTemplates = [];
      if (!workspace) return;
      readings += 1;
      const reading = readings;
      void workspace
        .templates()
        .then((all) => {
          if (reading === readings) {
            courseTemplates = all.filter((item) => item.kind === 'course');
          }
        })
        .catch(() => {
          if (reading === readings) {
            courseTemplates = [];
          }
        });
    });
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    const draft: CourseDraft = { title: title.trim(), type: alone ? 'P' : type };
    const chosen = selectedTemplateId
      ? courseTemplates.find((item) => item.id === selectedTemplateId)
      : undefined;
    open = false;
    onCreate(draft, chosen?.id);
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content data-testid="new-course-dialog" {@attach opened}>
    <form class="grid gap-4" onsubmit={submit}>
      <Dialog.Header>
        <Dialog.Title>{m.course_new_title()}</Dialog.Title>
        <Dialog.Description>{m.course_new_description()}</Dialog.Description>
      </Dialog.Header>
      {#if courseTemplates.length > 0}
        <div
          class="grid gap-2"
          role="group"
          aria-labelledby="{uid}-source"
          data-testid="new-course-source"
        >
          <span id="{uid}-source" class="font-medium">{m.course_new_start_with()}</span>
          <button
            type="button"
            aria-pressed={selectedTemplateId === ''}
            data-testid="new-course-empty"
            class={[
              'flex min-h-9 flex-col items-start rounded-md border px-3 py-2 text-left transition-colors roomy:min-h-touch',
              selectedTemplateId === ''
                ? 'border-primary bg-accent text-accent-foreground'
                : 'border-input-border hover:bg-accent/50',
            ]}
            onclick={() => {
              selectedTemplateId = '';
            }}
          >
            <span class="font-medium">{m.course_new_empty()}</span>
          </button>
          {#each courseTemplates as template (template.id)}
            {@const key =
              typeof template.properties.templateKey === 'string'
                ? template.properties.templateKey
                : template.id}
            {@const selected = selectedTemplateId === template.id}
            <button
              type="button"
              aria-pressed={selected}
              data-testid="new-course-template-{key}"
              class={[
                'flex min-h-9 flex-col items-start rounded-md border px-3 py-2 text-left transition-colors roomy:min-h-touch',
                selected
                  ? 'border-primary bg-accent text-accent-foreground'
                  : 'border-input-border hover:bg-accent/50',
              ]}
              onclick={() => {
                selectedTemplateId = template.id;
              }}
            >
              <span class="font-medium">{template.title}</span>
              {#if template.description}
                <span class="text-xs text-muted-foreground">{template.description}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
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
