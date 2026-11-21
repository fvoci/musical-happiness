<!--
  One diary, open on a day of the year (`/w/<workspace>/diaries/<diary>/<MM-DD>`):
  that day of every year, this year's on top and the years before it below,
  nearest first. Which diary and day comes from the address; this keeps the
  store and the address in step.

  A diary's address without a day opens today's, the address put in place
  (no entry of the history). A day that is no day of the year, a diary not
  there for the reader, and a board's identifier at a diary's address show
  the same "not found" as the rest of the app.

  The years go back to the one the diary was made in and to every year
  written on that day, and one more each time "Show <year>" is pressed (or a
  day of an earlier year is picked). One year is open at a time: as the page
  opens, this year's day if it has come (the reader's today, on their device);
  pressing another opens it instead. Midnight changes what today is, never
  which day is open: what is being written stays on its day.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import ConfirmDialog from '$lib/components/confirm/ConfirmDialog.svelte';
  import EntryBody from '$lib/features/card/EntryBody.svelte';
  import DayHeader from '$lib/features/diary/DayHeader.svelte';
  import DiaryNameDialog from '$lib/features/diary/DiaryNameDialog.svelte';
  import { DeviceToday } from '$lib/features/diary/today.svelte.ts';
  import YearCell from '$lib/features/diary/YearCell.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { dateOfInstant, type CalendarDate } from '$lib/logic/dates.ts';
  import {
    dateIn,
    monthDayOf,
    pageDays,
    parseMonthDay,
    stepMonthDay,
    writable,
    yearOf,
    type MonthDay,
  } from '$lib/logic/diary-days.ts';
  import { displayDay, displayYear } from '$lib/logic/format.ts';
  import { DiaryStore, whileHere, type EntryStore, type Workspace } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import NotFound from './NotFound.svelte';
  import type { Router } from './router.svelte.ts';

  let { workspace, router }: { workspace: Workspace; router: Router } = $props();

  const diaryId = $derived(router.route.diaryId);
  const asked = $derived(router.route.monthDay);
  const monthDay = $derived(asked === undefined ? undefined : parseMonthDay(asked));

  /** The diary of the address, made anew for another one. */
  const diary = $derived(diaryId ? new DiaryStore(workspace, diaryId) : undefined);
  const today = new DeviceToday(() => workspace.regional.timeZone);
  const locale = $derived(workspace.locale);

  // Shown while it is, closed when it goes. The board shown before lets go.
  $effect(() => {
    const store = diary;
    if (!store) return;
    untrack(() => {
      workspace.closeBoard();
    });
    return () => {
      store.close();
    };
  });

  // Without a day in the address: today's, in place of the diary's address.
  $effect(() => {
    if (diaryId && asked === undefined) {
      router.openDay(
        untrack(() => monthDayOf(today.date)),
        { replace: true },
      );
    }
  });

  // The day of the address is the one read.
  $effect(() => {
    const store = diary;
    const day = monthDay;
    if (store && day) untrack(() => void store.show(day));
  });

  /** The earliest year asked for, before the diary's first. */
  let reach = $state(Infinity);
  /** The year the diary was made in, on the reader's calendar. */
  const made = $derived(
    diary?.diary
      ? yearOf(dateOfInstant(diary.diary.createAt, workspace.regional.timeZone))
      : yearOf(today.date),
  );
  const from = $derived(Math.min(made, reach));
  const days = $derived(
    monthDay
      ? pageDays(
          monthDay,
          today.date,
          from,
          (diary?.entries ?? []).map((entry) => entry.date),
        )
      : [],
  );
  /**
   * The day of the year before the earliest shown, for "Show <year>": a leap
   * year's for `02-29`, also where no year shown has one.
   */
  const before = $derived.by(() => {
    if (!monthDay) return undefined;
    const below = Math.min(from, ...days.map(yearOf)) - 1;
    for (let year = below; year >= below - 8; year -= 1) {
      const date = dateIn(year, monthDay);
      if (date) return date;
    }
    return undefined;
  });

  /** This year's day, as the page opens on a day of the year: open if it has come. */
  const opening = $derived.by(() => {
    const day = monthDay;
    if (!day) return undefined;
    return untrack(() => {
      const date = dateIn(yearOf(today.date), day);
      return date && writable(date, today.date) ? date : undefined;
    });
  });
  /** The year the reader opened on this day of the year. */
  let chosen = $state.raw<{ monthDay: MonthDay; date: CalendarDate } | undefined>(undefined);
  /** The day the reader opened by pressing it: the caret goes into it. */
  let pressed = $state.raw<CalendarDate | undefined>(undefined);
  const open = $derived(chosen && chosen.monthDay === monthDay ? chosen.date : opening);

  function openDate(date: CalendarDate): void {
    chosen = { monthDay: monthDayOf(date), date };
  }

  function step(by: 1 | -1): void {
    if (monthDay) router.openDay(stepMonthDay(monthDay, by));
  }

  /** A day picked from the month: its day of the year, its year shown and open. */
  function pick(date: CalendarDate): void {
    reach = Math.min(reach, yearOf(date));
    openDate(date);
    router.openDay(monthDayOf(date));
  }

  function toToday(): void {
    openDate(today.date);
    router.openDay(monthDayOf(today.date));
  }

  let renaming = $state(false);
  let deleting = $state(false);
  const title = $derived(diary?.diary?.title || m.diary_untitled());

  async function rename(store: DiaryStore, name: string): Promise<void> {
    try {
      workspace.receiveContainer(
        await workspace.backend.patchContainer(store.diaryId, { title: name }),
      );
    } catch (error) {
      workspace.report('save', error);
    }
  }

  /** Deletes the diary; the page goes on to the first board, wiki or diary of the sidebar. */
  async function deleteDiary(store: DiaryStore): Promise<void> {
    const done = await whileHere(workspace, async () => {
      await workspace.deleteContainer(store.diaryId);
      return true;
    });
    if (!done || workspace.containers.some((left) => left.id === store.diaryId)) return;
    if (router.route.diaryId !== store.diaryId) return;
    const next = workspace.sidebar.flatMap((entry) => entry.containers)[0];
    if (next) router.openContainer(next);
    else router.go({ workspaceId: workspace.info?.id }, { replace: true });
  }
</script>

{#snippet body(entry: EntryStore, label: string)}
  <EntryBody {entry} {label} focus={pressed === entry.date} />
{/snippet}

{#if (asked !== undefined && !monthDay) || diary?.missing}
  <NotFound {router} />
{:else if !diary || !monthDay}
  <div class="grid h-full place-items-center p-6">
    <p class="text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
{:else}
  {@const store = diary}
  {@const day = monthDay}
  <div class="h-full overflow-y-auto" data-testid="diary-page" data-month-day={day}>
    <div class="mx-auto flex max-w-3xl flex-col gap-2 px-4 pt-4 pb-16 roomy:px-3">
      <DayHeader
        {title}
        monthDay={day}
        today={today.date}
        {locale}
        weekStart={workspace.regional.weekStart}
        onStep={step}
        onToday={toToday}
        onPick={pick}
        onRename={() => {
          renaming = true;
        }}
        onDelete={() => {
          deleting = true;
        }}
      />
      {#if day === '02-29'}
        <p class="text-sm text-muted-foreground">{m.diary_leap_only()}</p>
      {/if}
      <!-- The head stays as the days go by: what has the focus there keeps it. -->
      {#if store.status === 'failed'}
        <p class="py-6 text-center text-muted-foreground" role="alert">{m.app_problem_load()}</p>
      {:else if store.status !== 'ready'}
        <p class="py-6 text-center text-muted-foreground" role="status">{m.app_loading()}</p>
      {:else}
        <div
          aria-label={m.diary_years({ day: displayDay(`2000-${day}` as CalendarDate, locale) })}
          role="list"
          class="flex flex-col"
        >
          {#each days as date (date)}
            <div role="listitem" class="border-t first:border-t-0">
              <YearCell
                diary={store}
                {date}
                today={today.date}
                open={open === date}
                {locale}
                onOpen={() => {
                  openDate(date);
                  pressed = date;
                }}
                {body}
              />
            </div>
          {/each}
        </div>
        {#if before}
          {@const earlier = before}
          <Button
            variant="ghost"
            class="self-start text-muted-foreground roomy:min-h-touch"
            data-testid="diary-earlier"
            onclick={() => {
              reach = Math.min(reach, yearOf(earlier));
            }}
          >
            {m.diary_earlier_year({ year: displayYear(earlier, locale) })}
          </Button>
        {/if}
      {/if}
    </div>
  </div>
  <DiaryNameDialog
    bind:open={renaming}
    initial={store.diary?.title ?? ''}
    onSubmit={(name: string) => void rename(store, name)}
  />
  <ConfirmDialog
    bind:open={deleting}
    title={m.shell_delete_diary_title()}
    description={m.shell_delete_diary_info({ title })}
    confirmLabel={m['DeleteBoardDialog.confirm-delete']()}
    destructive
    onConfirm={() => void deleteDiary(store)}
  />
{/if}
