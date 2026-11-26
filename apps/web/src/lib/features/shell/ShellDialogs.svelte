<!--
  Dialogs and overlays owned by `AppShell.svelte`: file transfers, template
  picker, new wiki/course dialogs, account settings, activity records, and dev readout.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';

  import { nameOfWorkspace } from '$lib/components/workspace-name.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { ContainerPlace } from '$lib/logic/urls.ts';
  import type { Container } from '$lib/logic/wire.ts';
  import {
    isOwnSpace,
    type CourseDraft,
    type WikiDraft,
    type Workspace,
  } from '$lib/state/index.ts';
  import type { AuditRecord } from '$lib/state/organization.ts';

  import AuditDialog from './AuditDialog.svelte';
  import { devReadout } from './dev.svelte.ts';
  import DevReadout from './DevReadout.svelte';
  import NewCourseDialog from './NewCourseDialog.svelte';
  import NewWikiDialog from './NewWikiDialog.svelte';
  import SettingsDialog from './SettingsDialog.svelte';
  import TemplatePicker from './TemplatePicker.svelte';
  import Transfers from './Transfers.svelte';

  interface Props {
    workspace: Workspace;
    picking: boolean;
    namingWiki: boolean;
    namingCourse: boolean;
    settingsOpen: boolean;
    recordsOpen: boolean;
    worksHere: boolean;
    locales?: readonly string[] | undefined;
    language?: string | undefined;
    autoLocale?: string | undefined;
    onChooseLanguage?: ((setting: string) => void) | undefined;
    onSignOut?: (() => void) | undefined;
    onChangePassword?: ((oldPassword: string, newPassword: string) => Promise<void>) | undefined;
    accountSettings?: Snippet | undefined;
    containerHref: (container: ContainerPlace) => string;
    onOpenContainer: (container: ContainerPlace) => void;
    onEmptyBoard: () => void;
    onFromTemplate: (template: Container) => void;
    onAddWiki: (draft: WikiDraft) => void;
    onAddCourse: (draft: CourseDraft, templateId?: string) => void;
    readRecords?: ((workspaceId: string) => Promise<AuditRecord[]>) | undefined;
  }

  let {
    workspace,
    picking = $bindable(),
    namingWiki = $bindable(),
    namingCourse = $bindable(),
    settingsOpen = $bindable(),
    recordsOpen = $bindable(),
    worksHere,
    locales,
    language,
    autoLocale,
    onChooseLanguage,
    onSignOut,
    onChangePassword,
    accountSettings,
    containerHref,
    onOpenContainer,
    onEmptyBoard,
    onFromTemplate,
    onAddWiki,
    onAddCourse,
    readRecords,
  }: Props = $props();
</script>

<Transfers {workspace} />

<TemplatePicker
  bind:open={picking}
  {workspace}
  onNewWiki={workspace.backend.wiki
    ? () => {
        namingWiki = true;
      }
    : undefined}
  onNewCourse={workspace.backend.course
    ? () => {
        namingCourse = true;
      }
    : undefined}
  {onEmptyBoard}
  {onFromTemplate}
  onEditTemplate={onOpenContainer}
/>

<NewWikiDialog
  bind:open={namingWiki}
  workspaceName={nameOfWorkspace(workspace.info, m.shell_workspace_unnamed)}
  alone={isOwnSpace(workspace.info)}
  onCreate={onAddWiki}
/>

<NewCourseDialog
  bind:open={namingCourse}
  {workspace}
  workspaceName={nameOfWorkspace(workspace.info, m.shell_workspace_unnamed)}
  alone={isOwnSpace(workspace.info)}
  onCreate={onAddCourse}
/>

<SettingsDialog
  bind:open={settingsOpen}
  {workspace}
  {locales}
  {language}
  {autoLocale}
  {onChooseLanguage}
  {onSignOut}
  {onChangePassword}
  account={accountSettings}
  {containerHref}
  onOpenContainer={(container: ContainerPlace) => {
    settingsOpen = false;
    onOpenContainer(container);
  }}
  onOpenRecords={readRecords && worksHere
    ? () => {
        settingsOpen = false;
        recordsOpen = true;
      }
    : undefined}
/>

{#if readRecords}
  <AuditDialog
    bind:open={recordsOpen}
    workspaceId={workspace.info?.id}
    locale={workspace.locale}
    {readRecords}
  />
{/if}

{#if devReadout.shown}
  <DevReadout />
{/if}
