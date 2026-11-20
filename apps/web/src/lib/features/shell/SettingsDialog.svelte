<!--
  The reader's settings. Every change is saved at once
  (`workspace.changeSettings`): it shows immediately and goes back, with a
  line telling so, if the server refuses it.

  - Theme, density, haptics: how the app looks and feels here. The line
    numbers of code blocks are this device's alone (code-lines.ts).
  - How a card opens: only the dialog in the middle is built. The other two
    answers are shown, so that the place is known, and cannot be chosen.
  - Time zone, first day of the week, way of writing hours: `Automatic`
    follows the device and the language.
  - Language: an account setting, `Automatic` following the browser; the page
    loads again in the chosen one. The list and the function that changes it
    come from the app (a feature cannot import them).
  - The layout readout: for whoever works on the layout (dev.svelte.ts).
  - Archives: every board and wiki of the workspace exported, an archive imported; the
    progress shows at the foot of the page (`Transfers.svelte`). An import that
    left members out, who are not in the workspace, says how many in a line.
  - The workspace's records, which its members read (`AuditDialog.svelte`): only
    where the backend has them.
  - The name: its parts, which of them others see, the title and credentials
    (`NameFields.svelte`).
  - Who is signed in, signing out and changing the password: only where the
    backend has accounts.

  A dialog in the middle of the page; on a compact page it fills the screen,
  clear of the edges the device keeps. The back button closes it.
-->
<script lang="ts">
  import DownloadIcon from '@lucide/svelte/icons/download';
  import UploadIcon from '@lucide/svelte/icons/upload';
  import XIcon from '@lucide/svelte/icons/x';
  import type { Snippet } from 'svelte';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import { codeLines } from '$lib/editor/code-lines.svelte.ts';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { closeOnBack, isOverlayEntry } from '$lib/layout/back.svelte.ts';
  import { userDisplayName } from '$lib/logic/users.ts';
  import { page } from '$lib/layout/room.svelte.ts';
  import {
    THEME_SETTINGS,
    CARD_OPEN_BUILT,
    CARD_OPEN_SETTINGS,
    DENSITY_SETTINGS,
    type ThemeSetting,
    type CardOpenSetting,
    type DensitySetting,
  } from '$lib/logic/display.ts';
  import {
    HOUR_CYCLE_SETTINGS,
    WEEK_START_SETTINGS,
    type HourCycleSetting,
    type WeekStartSetting,
  } from '$lib/logic/regional.ts';
  import { browserTimeZone } from '$lib/regional.ts';
  import { saveBlob } from '$lib/save.ts';
  import { isGuest, type Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';
  import * as Dialog from '$lib/ui/dialog/index.js';
  import { Switch } from '$lib/ui/switch/index.js';

  import { devReadout } from './dev.svelte.ts';
  import NameFields from './NameFields.svelte';
  import PasswordChange from './PasswordChange.svelte';
  import SettingChoice from './SettingChoice.svelte';
  import SettingPicker from './SettingPicker.svelte';
  import {
    AUTO,
    languageName,
    supportedTimeZones,
    timeZoneChoices,
    timeZoneLabel,
  } from './settings-logic.ts';

  interface Props {
    open: boolean;
    workspace: Workspace;
    /** The languages the app has, as tags. Without them the language is not offered. */
    locales?: readonly string[];
    /** The reader's language setting: `auto`, or the one of `locales` it is shown in. */
    language?: string;
    /** The one of `locales` that `auto` shows the app in. */
    autoLocale?: string;
    /** Changes the language setting; the page is loaded again in it. */
    onChooseLanguage?: (setting: string) => void;
    /** Signs the reader out. Without it there is no account to leave (the backend in memory). */
    onSignOut?: () => void;
    /** Changes the password. Without it there is no account (the backend in memory). */
    onChangePassword?: (oldPassword: string, newPassword: string) => Promise<void>;
    /** What else is the account's, below the password: the app's to fill. */
    account?: Snippet;
    /** Opens the workspace's records. Without it (no server) they are not offered. */
    onOpenRecords?: (() => void) | undefined;
  }

  let {
    open = $bindable(),
    workspace,
    locales = [],
    language,
    autoLocale,
    onChooseLanguage,
    onSignOut,
    onChangePassword,
    account,
    onOpenRecords,
  }: Props = $props();

  closeOnBack(
    () => open,
    () => {
      open = false;
      leftOut = undefined;
    },
  );

  const workspaceName = $derived(workspace.info?.title || m.shell_workspace_unnamed());

  const uid = $props.id();
  let importField = $state<HTMLInputElement | null>(null);
  /**
   * What the last import from here left out, and in which workspace: told
   * while the dialog stays open on that workspace, forgotten when it closes.
   */
  let leftOut = $state<{ workspaceId: string; count: number }>();
  const leftOutHere = $derived(
    leftOut?.workspaceId === workspace.info?.id ? (leftOut?.count ?? 0) : 0,
  );

  /** Saves an archive of every board of the workspace, once it has all arrived. */
  async function exportWorkspace(): Promise<void> {
    const archive = await workspace.exportWorkspace();
    if (archive) saveBlob(archive.bytes, archive.name);
  }

  const settings = $derived(workspace.settings);
  const compact = $derived(page.stage === 'compact');

  const THEME_LABELS: Record<ThemeSetting, () => string> = {
    auto: m.shell_settings_auto,
    light: m.shell_settings_theme_light,
    dark: m.shell_settings_theme_dark,
  };
  const CARD_OPEN_LABELS: Record<CardOpenSetting, () => string> = {
    center: m.shell_settings_card_open_center,
    side: m.shell_settings_card_open_side,
    full: m.shell_settings_card_open_full,
  };
  const DENSITY_LABELS: Record<DensitySetting, () => string> = {
    auto: m.shell_settings_auto,
    compact: m.shell_settings_density_compact,
    comfortable: m.shell_settings_density_comfortable,
  };
  const WEEK_START_LABELS: Record<WeekStartSetting, () => string> = {
    auto: m.shell_settings_auto,
    sunday: m.shell_settings_week_start_sunday,
    monday: m.shell_settings_week_start_monday,
    saturday: m.shell_settings_week_start_saturday,
  };
  const HOUR_CYCLE_LABELS: Record<HourCycleSetting, () => string> = {
    auto: m.shell_settings_auto,
    h12: m.shell_settings_hour_cycle_h12,
    h23: m.shell_settings_hour_cycle_h23,
  };

  const timeZones = $derived([
    { id: AUTO, label: m.shell_settings_time_zone_auto({ timeZone: browserTimeZone() }) },
    ...timeZoneChoices(supportedTimeZones(), settings.timeZone).map((zone) => ({
      id: zone,
      label: timeZoneLabel(zone),
      also: [zone],
    })),
  ]);

  const languages = $derived([
    {
      id: AUTO,
      label: m.shell_settings_language_auto({ language: languageName(autoLocale ?? '') }),
    },
    ...locales.map((tag) => ({ id: tag, label: languageName(tag), also: [tag] })),
  ]);

  /** How long the change of language waits for the history at most, in milliseconds. */
  const GIVE_BACK_LIMIT = 2000;

  /**
   * Changes the language once the dialog (and the list over it) have given
   * their entries of the history back: the page is loaded again right after,
   * and entries left behind would make "back" do nothing a few times.
   */
  function chooseLanguage(tag: string): void {
    open = false;
    const started = performance.now();
    const whenGivenBack = (): void => {
      // An entry of a layer is still the current one: the steps back are on their way.
      if (isOverlayEntry(history.state) && performance.now() - started < GIVE_BACK_LIMIT) {
        requestAnimationFrame(whenGivenBack);
      } else {
        onChooseLanguage?.(tag);
      }
    };
    requestAnimationFrame(whenGivenBack);
  }
</script>

<Dialog.Root
  bind:open
  onOpenChange={(now: boolean) => {
    if (!now) leftOut = undefined;
  }}
>
  <Dialog.Content
    data-testid="settings-dialog"
    data-frame={compact ? 'full' : 'dialog'}
    showCloseButton={false}
    class={[
      'flex flex-col gap-0 p-0',
      compact
        ? 'top-0 left-0 h-dvh max-w-none translate-x-0 translate-y-0 rounded-none pt-(--safe-top) pr-(--safe-right) pb-(--safe-bottom) pl-(--safe-left) ring-0 sm:max-w-none'
        : 'left-[calc(50%+(var(--safe-left)-var(--safe-right))/2)] max-h-[calc(100dvh-2rem-var(--safe-top)-var(--safe-bottom))] w-[min(28rem,calc(100%-2rem-var(--safe-left)-var(--safe-right)))] max-w-none sm:max-w-none',
    ]}
  >
    <Dialog.Header class="shrink-0 flex-row items-start gap-2 border-b py-2 pr-2 pl-4">
      <div class="min-w-0 flex-1 py-1">
        <Dialog.Title class="pr-0 text-base font-semibold">{m['Sidebar.settings']()}</Dialog.Title>
        <Dialog.Description class="text-muted-foreground">
          {m.shell_settings_description()}
        </Dialog.Description>
      </div>
      <Dialog.Close>
        {#snippet child({ props }: { props: Record<string, unknown> })}
          <IconButton {...props} label={m.ui_close()} data-testid="close-settings">
            <XIcon />
          </IconButton>
        {/snippet}
      </Dialog.Close>
    </Dialog.Header>

    <div class="grid min-h-0 flex-1 content-start gap-5 overflow-y-auto overscroll-contain p-4">
      <SettingChoice
        name="theme"
        label={m.shell_settings_theme()}
        value={settings.theme}
        options={THEME_SETTINGS.map((value) => ({ value, label: THEME_LABELS[value]() }))}
        onChange={(theme: ThemeSetting) => void workspace.changeSettings({ theme })}
      />
      <SettingChoice
        name="density"
        label={m.shell_settings_density()}
        value={settings.density}
        options={DENSITY_SETTINGS.map((value) => ({ value, label: DENSITY_LABELS[value]() }))}
        onChange={(density: DensitySetting) => void workspace.changeSettings({ density })}
      />
      <SettingChoice
        name="cardOpen"
        label={m.shell_settings_card_open()}
        value={settings.cardOpen}
        options={CARD_OPEN_SETTINGS.map((value) => {
          const built = CARD_OPEN_BUILT.includes(value);
          return {
            value,
            label: CARD_OPEN_LABELS[value](),
            disabled: !built,
            note: built ? undefined : m.shell_settings_coming(),
          };
        })}
        onChange={(cardOpen: CardOpenSetting) => void workspace.changeSettings({ cardOpen })}
      />
      <label class="flex min-h-8 items-center gap-3 roomy:min-h-touch" data-setting="haptics">
        <span class="grid min-w-0 flex-1 gap-0.5">
          <span class="font-medium">{m.shell_settings_haptics()}</span>
          <span class="text-xs text-muted-foreground">{m.shell_settings_haptics_hint()}</span>
        </span>
        <Switch
          checked={settings.haptics}
          onCheckedChange={(haptics: boolean) => void workspace.changeSettings({ haptics })}
        />
      </label>
      <label class="flex min-h-8 items-center gap-3 roomy:min-h-touch" data-setting="codeLines">
        <span class="grid min-w-0 flex-1 gap-0.5">
          <span class="font-medium">{m.shell_settings_code_lines()}</span>
          <span class="text-xs text-muted-foreground">{m.shell_settings_code_lines_hint()}</span>
        </span>
        <Switch
          checked={codeLines.shown}
          onCheckedChange={(shown: boolean) => {
            codeLines.set(shown);
          }}
        />
      </label>

      <SettingPicker
        name="timeZone"
        label={m.shell_settings_time_zone()}
        searchLabel={m.shell_settings_time_zone_search()}
        items={timeZones}
        value={settings.timeZone}
        onPick={(timeZone: string) => void workspace.changeSettings({ timeZone })}
      />
      <SettingChoice
        name="weekStart"
        label={m.shell_settings_week_start()}
        value={settings.weekStart}
        options={WEEK_START_SETTINGS.map((value) => ({ value, label: WEEK_START_LABELS[value]() }))}
        onChange={(weekStart: WeekStartSetting) => void workspace.changeSettings({ weekStart })}
      />
      <SettingChoice
        name="hourCycle"
        label={m.shell_settings_hour_cycle()}
        value={settings.hourCycle}
        options={HOUR_CYCLE_SETTINGS.map((value) => ({ value, label: HOUR_CYCLE_LABELS[value]() }))}
        onChange={(hourCycle: HourCycleSetting) => void workspace.changeSettings({ hourCycle })}
      />

      {#if locales.length > 0 && language !== undefined && onChooseLanguage}
        <SettingPicker
          name="language"
          label={m.shell_settings_language()}
          searchLabel={m.shell_settings_language_search()}
          items={languages}
          value={language}
          onPick={chooseLanguage}
        />
      {/if}

      <label
        class="flex min-h-8 items-center gap-3 border-t pt-4 roomy:min-h-touch"
        data-setting="dev"
      >
        <span class="grid min-w-0 flex-1 gap-0.5">
          <span class="font-medium">{m.shell_settings_dev()}</span>
          <span class="text-xs text-muted-foreground">{m.shell_settings_dev_hint()}</span>
        </span>
        <Switch
          checked={devReadout.shown}
          onCheckedChange={(shown: boolean) => {
            devReadout.set(shown);
          }}
        />
      </label>

      <section class="grid gap-2 border-t pt-4" aria-labelledby="{uid}-archives">
        <h3 id="{uid}-archives" class="font-medium">{m.shell_archive_title()}</h3>
        <div class="flex flex-wrap gap-2">
          <Button
            variant="outline"
            class="roomy:min-h-touch"
            data-testid="export-workspace"
            onclick={() => void exportWorkspace()}
          >
            <DownloadIcon aria-hidden="true" />
            {m.shell_export_workspace()}
          </Button>
          <!-- A guest brings no boards in: importing makes boards. -->
          {#if !isGuest(workspace)}
            <Button
              variant="outline"
              class="roomy:min-h-touch"
              aria-describedby="{uid}-import-hint"
              onclick={() => importField?.click()}
            >
              <UploadIcon aria-hidden="true" />
              {m.shell_import()}
            </Button>
            <input
              bind:this={importField}
              type="file"
              accept=".boardarchive,application/zip"
              hidden
              data-testid="import-archive"
              onchange={(event) => {
                const field = event.currentTarget;
                const file = field.files?.[0];
                // The same file can be chosen again.
                field.value = '';
                const workspaceId = workspace.info?.id;
                if (!file || workspaceId === undefined) return;
                leftOut = undefined;
                void workspace.importArchive(file).then((imported) => {
                  leftOut = { workspaceId, count: imported?.membersLeftOut ?? 0 };
                });
              }}
            />
          {/if}
        </div>
        {#if !isGuest(workspace)}
          <p id="{uid}-import-hint" class="text-xs text-muted-foreground">
            {m.shell_import_hint({ workspace: workspaceName })}
          </p>
        {/if}
        <!-- There before it says anything, so that what it says is announced. -->
        <p
          role="status"
          class={leftOutHere > 0 ? 'text-xs' : 'sr-only'}
          data-testid="import-left-out"
        >
          {#if leftOutHere > 0}{m.shell_import_left_out({
              count: leftOutHere,
              workspace: workspaceName,
            })}{/if}
        </p>
      </section>

      {#if onOpenRecords}
        <section class="grid gap-2 border-t pt-4" aria-labelledby="{uid}-records">
          <h3 id="{uid}-records" class="font-medium">{m.shell_settings_workspace()}</h3>
          <div>
            <Button
              variant="outline"
              class="roomy:min-h-touch"
              aria-haspopup="dialog"
              data-testid="open-records"
              onclick={onOpenRecords}>{m.audit_title()}</Button
            >
          </div>
          <p class="text-xs text-muted-foreground">{m.audit_description()}</p>
        </section>
      {/if}

      {#if workspace.me}
        <div class="border-t pt-4">
          <NameFields {workspace} />
        </div>
      {/if}

      {#if onChangePassword}
        <PasswordChange onChange={onChangePassword} />
      {/if}

      {@render account?.()}

      {#if onSignOut}
        <div class="flex min-h-8 items-center gap-3 border-t pt-4 roomy:min-h-touch">
          <span class="min-w-0 flex-1 truncate text-muted-foreground" data-testid="signed-in-as">
            {m.account_signed_in_as({
              // The reader is not always among the people read of where they
              // are (somebody new there, before they are on a board).
              name: workspace.me ? userDisplayName(workspace.me) : '',
            })}
          </span>
          <Button variant="outline" class="roomy:min-h-touch" onclick={onSignOut}>
            {m.account_sign_out()}
          </Button>
        </div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
