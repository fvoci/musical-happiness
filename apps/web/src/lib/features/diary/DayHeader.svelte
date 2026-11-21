<!--
  The head of a diary's page: the diary's name, the day of the year shown
  (`November 21`), the way to the day before and the day after, back to
  today, and to any day picked from a month (its year shown too, below this
  year's), and the diary's menu (renaming it, deleting it).

  "Today" is the reader's: it says it is the day shown (`aria-current`) and
  takes them back there from any other, also after midnight came while the
  page was open.
-->
<script lang="ts">
  import { parseDate, type DateValue as PickedDay } from '@internationalized/date';
  import CalendarIcon from '@lucide/svelte/icons/calendar-days';
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import IconButton from '$lib/components/icon-button/IconButton.svelte';
  import ActionMenu from '$lib/components/menu/ActionMenu.svelte';
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { insets } from '$lib/layout/room.svelte.ts';
  import type { CalendarDate } from '$lib/logic/dates.ts';
  import { dateIn, monthDayOf, yearOf, type MonthDay } from '$lib/logic/diary-days.ts';
  import { displayDay } from '$lib/logic/format.ts';
  import { Button } from '$lib/ui/button/index.js';
  import { Calendar } from '$lib/ui/calendar/index.js';
  import * as Popover from '$lib/ui/popover/index.js';

  interface Props {
    /** The diary's name as it is shown. */
    title: string;
    monthDay: MonthDay;
    today: CalendarDate;
    locale: string;
    /** The first day of the week, 0 for Sunday. */
    weekStart: number;
    onStep: (by: 1 | -1) => void;
    onToday: () => void;
    /** A day was picked, of any year up to today. */
    onPick: (date: CalendarDate) => void;
    onRename: () => void;
    onDelete: () => void;
  }

  let {
    title,
    monthDay,
    today,
    locale,
    weekStart,
    onStep,
    onToday,
    onPick,
    onRename,
    onDelete,
  }: Props = $props();

  /** The day of the year shown, written for this leap year: `02-29` is one of them. */
  const shown = $derived(`2000-${monthDay}` as CalendarDate);
  const isToday = $derived(monthDayOf(today) === monthDay);
  let picking = $state(false);
  /** The picker opens on this year's day shown, or today where this year has none (`02-29`). */
  const placeholder = $derived(parseDate(dateIn(yearOf(today), monthDay) ?? today));

  function pick(picked: PickedDay | undefined): void {
    if (!picked) return;
    picking = false;
    onPick(picked.toString() as CalendarDate);
  }

  const actions = $derived([
    { id: 'rename', label: m.diary_rename(), icon: PencilIcon, run: onRename },
    {
      id: 'delete',
      label: m.shell_delete_diary(),
      icon: Trash2Icon,
      destructive: true,
      separated: true,
      run: onDelete,
    },
  ]);
</script>

<header class="flex flex-col gap-1 border-b pb-3" data-testid="diary-head">
  <div class="flex items-center gap-2">
    <p class="min-w-0 flex-1 truncate text-sm text-muted-foreground" data-testid="diary-title">
      {title}
    </p>
    <ActionMenu title={m.diary_menu({ title })} {actions} align="end">
      {#snippet trigger(props: Record<string, unknown>)}
        <IconButton {...props} label={m.diary_menu({ title })} data-testid="diary-menu">
          <EllipsisIcon />
        </IconButton>
      {/snippet}
    </ActionMenu>
  </div>
  <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
    <h1 class="min-w-0 flex-1 text-2xl font-semibold tracking-tight" data-testid="diary-day">
      {displayDay(shown, locale)}
    </h1>
    <div class="flex items-center gap-1">
      <IconButton
        label={m.diary_previous_day()}
        data-testid="diary-previous"
        onclick={() => {
          onStep(-1);
        }}
      >
        <ChevronLeftIcon />
      </IconButton>
      <Button
        variant="outline"
        size="sm"
        class="roomy:min-h-touch"
        aria-current={isToday ? 'date' : undefined}
        data-testid="diary-today"
        onclick={onToday}
      >
        {m.diary_today()}
      </Button>
      <IconButton
        label={m.diary_next_day()}
        data-testid="diary-next"
        onclick={() => {
          onStep(1);
        }}
      >
        <ChevronRightIcon />
      </IconButton>
      <Popover.Root bind:open={picking}>
        <Popover.Trigger>
          {#snippet child({ props }: { props: Record<string, unknown> })}
            <IconButton {...props} label={m.diary_pick_day()} data-testid="diary-pick">
              <CalendarIcon />
            </IconButton>
          {/snippet}
        </Popover.Trigger>
        <Popover.Content
          align="end"
          collisionPadding={insets.collisionPadding}
          aria-label={m.diary_pick_day()}
          class="w-auto p-1"
          data-testid="diary-picker"
        >
          <Calendar
            type="single"
            bind:value={() => undefined, pick}
            {placeholder}
            {locale}
            weekStartsOn={(weekStart % 7) as 0 | 1 | 2 | 3 | 4 | 5 | 6}
            maxValue={parseDate(today)}
            fixedWeeks
            calendarLabel={m.diary_pick_day()}
            class="bg-transparent p-1 roomy:[--cell-size:var(--spacing-touch)]"
          />
        </Popover.Content>
      </Popover.Root>
    </div>
  </div>
</header>
