<!--
  "New": a wiki (`onNewWiki`, where the backend has wikis), an empty board,
  or a board made from a template. The
  templates are read each time the dialog opens: the workspace's (which can be
  opened to be changed) and then the built-in ones every workspace is offered.
  A template is changed where any board is: on its own page.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import BookOpenIcon from '@lucide/svelte/icons/book-open';
  import GraduationCapIcon from '@lucide/svelte/icons/graduation-cap';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import type { Container } from '$lib/logic/wire.ts';
  import type { Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import ContainerIcon from './ContainerIcon.svelte';

  interface Props {
    open: boolean;
    workspace: Workspace;
    onEmptyBoard: () => void;
    onFromTemplate: (template: Container) => void;
    /** Opens a template of the workspace on its page, to be changed. */
    onEditTemplate: (template: Container) => void;
    /** Asks for a new wiki; left out where there are none. */
    onNewWiki?: (() => void) | undefined;
    /** Asks for a new course; left out where there are none. */
    onNewCourse?: (() => void) | undefined;
  }

  let {
    open = $bindable(),
    workspace,
    onEmptyBoard,
    onFromTemplate,
    onEditTemplate,
    onNewWiki,
    onNewCourse,
  }: Props = $props();

  const compact = $derived(page.stage === 'compact');
  let templates = $state.raw<Container[] | undefined>(undefined);
  /** Readings of the templates begun: only the latest is shown. */
  let readings = 0;

  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );

  /** Each time the dialog opens (its content is made): the templates read anew. */
  function opened(): void {
    untrack(() => {
      templates = undefined;
      readings += 1;
      const reading = readings;
      void workspace.templates().then((read) => {
        if (reading === readings) templates = read;
      });
    });
  }

  const titleOf = (board: Container): string => board.title || m['View.NewTemplateDefaultTitle']();
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    {@attach opened}
    data-testid="template-picker"
    showCloseButton={false}
    class={[
      'flex flex-col gap-0 p-0',
      compact
        ? 'top-0 left-0 h-dvh max-w-none translate-x-0 translate-y-0 rounded-none pt-(--safe-top) pr-(--safe-right) pb-(--safe-bottom) pl-(--safe-left) ring-0 sm:max-w-none'
        : 'max-h-[calc(100dvh-2rem-var(--safe-top)-var(--safe-bottom))] w-[min(36rem,calc(100%-2rem))] max-w-none sm:max-w-none',
    ]}
  >
    <Dialog.Header class="shrink-0 flex-row items-start gap-2 border-b py-2 pr-2 pl-4">
      <div class="min-w-0 flex-1 py-1">
        <Dialog.Title class="text-base font-semibold">
          {m['BoardTemplateSelector.title']()}
        </Dialog.Title>
        <Dialog.Description class="text-muted-foreground">
          {m['BoardTemplateSelector.description']()}
        </Dialog.Description>
      </div>
      <Dialog.Close>
        {#snippet child({ props }: { props: Record<string, unknown> })}
          <IconButton {...props} label={m.ui_close()}>
            <XIcon />
          </IconButton>
        {/snippet}
      </Dialog.Close>
    </Dialog.Header>

    <div class="grid min-h-0 flex-1 content-start gap-3 overflow-y-auto overscroll-contain p-4">
      <Button
        variant="outline"
        class="justify-start roomy:min-h-touch"
        data-testid="empty-board"
        onclick={() => {
          open = false;
          onEmptyBoard();
        }}
      >
        <PlusIcon aria-hidden="true" />
        {m['BoardTemplateSelector.create-empty-board']()}
      </Button>
      {#if onNewWiki}
        <Button
          variant="outline"
          class="justify-start roomy:min-h-touch"
          data-testid="new-wiki"
          onclick={() => {
            open = false;
            onNewWiki();
          }}
        >
          <BookOpenIcon aria-hidden="true" />
          {m.wiki_new_title()}
        </Button>
      {/if}
      {#if onNewCourse}
        <Button
          variant="outline"
          class="justify-start roomy:min-h-touch"
          data-testid="new-course"
          onclick={() => {
            open = false;
            onNewCourse();
          }}
        >
          <GraduationCapIcon aria-hidden="true" />
          {m.course_new_title()}
        </Button>
      {/if}

      {#if templates === undefined}
        <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
      {:else}
        <ul class="grid gap-1" data-testid="templates">
          {#each templates as template (template.id)}
            {@const title = titleOf(template)}
            <li class="flex items-start gap-2 rounded-md p-2 hover:bg-accent" data-template={title}>
              <ContainerIcon icon={template.icon} {title} locale={workspace.locale} />
              <div class="min-w-0 flex-1">
                <p class="truncate font-medium">{title}</p>
                {#if template.description}
                  <p class="line-clamp-2 text-xs text-muted-foreground">
                    {template.description}
                  </p>
                {/if}
              </div>
              {#if !template.isBuiltin}
                <IconButton
                  label={m.shell_template_edit({ name: title })}
                  class="text-muted-foreground"
                  onclick={() => {
                    open = false;
                    onEditTemplate(template);
                  }}
                >
                  <PencilIcon />
                </IconButton>
              {/if}
              <Button
                variant="outline"
                size="sm"
                class="shrink-0 roomy:min-h-touch"
                aria-label={m.shell_template_use({ name: title })}
                onclick={() => {
                  open = false;
                  onFromTemplate(template);
                }}
              >
                {m['BoardTemplateSelector.use-this-template']()}
              </Button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
