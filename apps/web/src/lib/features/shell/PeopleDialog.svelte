<!--
  The people of the workspace, in a dialog the sidebar opens for who administers
  it. What it shows is `PeoplePanel.svelte`, made anew each time it opens.
-->
<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack } from '$lib/layout/back.svelte.ts';
  import type { People, Workspace } from '$lib/state/index.ts';
  import * as Dialog from '$lib/ui/dialog/index.js';

  import PeoplePanel from './PeoplePanel.svelte';

  interface Props {
    open: boolean;
    workspace: Workspace;
    people: People;
    /** The whole address of the page of an invitation, by its token. */
    inviteLink: (token: string) => string;
  }

  let { open = $bindable(), workspace, people, inviteLink }: Props = $props();

  const workspaceName = $derived(workspace.info?.title || m.shell_workspace_unnamed());

  // The back button closes it, as the other dialogs: the page under it stays.
  closeOnBack(
    () => open,
    () => {
      open = false;
    },
  );
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    class="flex max-h-[min(90dvh,44rem)] flex-col gap-0 p-0 sm:max-w-lg"
    showCloseButton={false}
    data-testid="people-dialog"
  >
    <Dialog.Header class="flex-row items-center gap-2 border-b p-4">
      <div class="min-w-0 flex-1">
        <Dialog.Title>{m.people_title()}</Dialog.Title>
        <Dialog.Description>{m.people_description({ workspace: workspaceName })}</Dialog.Description
        >
      </div>
      <Dialog.Close>
        {#snippet child({ props }: { props: Record<string, unknown> })}
          <IconButton {...props} label={m.ui_close()}>
            <XIcon />
          </IconButton>
        {/snippet}
      </Dialog.Close>
    </Dialog.Header>
    {#if open}
      <PeoplePanel {workspace} {people} {inviteLink} />
    {/if}
  </Dialog.Content>
</Dialog.Root>
