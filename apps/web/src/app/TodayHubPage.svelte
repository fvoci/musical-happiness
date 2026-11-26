<!--
  Unified account-wide `'Today'` hub (`/today`): shows cards (overdue, due today,
  due soon), scheduled learning units, due reviews, upcoming study meetings
  across session-opened workspaces, plus personal-diary streak via `getDiaryRuns`.
-->
<script lang="ts">
  import { untrack } from 'svelte';

  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { displayDateTime } from '$lib/logic/format.ts';
  import {
    syncDeviceTimeZone,
    TodayHubStore,
    type TodayCardItem,
    type TodayMeetingItem,
    type TodayReviewEntry,
    type TodayUnitEntry,
    type TodayWorkspaceMeta,
    type Workspace,
  } from '$lib/state/index.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { goToPlace } from './places.ts';
  import type { Router } from './router.svelte.ts';

  interface Props {
    workspace: Workspace;
    router: Router;
  }

  let { workspace, router }: Props = $props();

  const store = $derived(new TodayHubStore(workspace));

  $effect(() => {
    const current = store;
    return () => {
      current.close();
    };
  });

  $effect(() => {
    const current = store;
    const zone = workspace.settings.timeZone;
    untrack(() => {
      if (zone === 'auto') {
        void syncDeviceTimeZone(workspace).then((changed) => {
          if (!changed) void current.load();
        });
      } else {
        void current.load();
      }
    });
  });

  const data = $derived(store.data);
  const overdueCards = $derived((data?.cards ?? []).filter((c) => c.overdueDays > 0));
  const todayCards = $derived(
    (data?.cards ?? []).filter((c) => c.overdueDays === 0 && c.dueDate === data?.today),
  );
  const soonCards = $derived(
    (data?.cards ?? []).filter((c) => c.overdueDays === 0 && c.dueDate !== data?.today),
  );
  const isEmpty = $derived(
    data?.cards.length === 0 &&
      data.units.length === 0 &&
      data.reviews.length === 0 &&
      data.meetings.length === 0,
  );

  function wsBadge(meta: TodayWorkspaceMeta): string {
    return meta.isPersonal ? m.own_space_name() : meta.workspaceName || m.shell_workspace_unnamed();
  }

  function openCardItem(item: TodayCardItem): void {
    goToPlace(router, {
      kind: 'card',
      id: item.cardId,
      containerId: item.boardId,
      workspaceId: item.workspace.workspaceId,
    });
  }

  function openUnitItem(item: TodayUnitEntry): void {
    goToPlace(router, {
      kind: 'unit',
      id: item.unitId,
      containerId: item.courseId,
      workspaceId: item.workspace.workspaceId,
    });
  }

  function openReviewItem(item: TodayReviewEntry): void {
    goToPlace(router, {
      kind: 'unit',
      id: item.unitId,
      containerId: item.containerId,
      workspaceId: item.workspace.workspaceId,
    });
  }

  function openMeetingItem(item: TodayMeetingItem): void {
    goToPlace(router, {
      kind: 'meeting',
      id: item.meetingId,
      containerId: item.containerId,
      workspaceId: item.workspace.workspaceId,
    });
  }
</script>

<div
  class="h-full overflow-y-auto overscroll-contain p-4 @columns/pane:p-6"
  data-testid="today-hub-page"
>
  <div class="mx-auto grid max-w-4xl gap-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold">{m.today_hub_title()}</h2>
        <p class="text-sm text-muted-foreground">{m.today_hub_subtitle()}</p>
      </div>
      <Button
        variant="outline"
        size="sm"
        class="roomy:min-h-touch"
        disabled={store.status === 'loading'}
        onclick={() => void store.load()}
        data-testid="today-hub-refresh"
      >
        <RefreshCwIcon class="size-4" aria-hidden="true" />
        {m.learning_today_refresh()}
      </Button>
    </div>

    {#if data && data.lockedWorkspaceCount > 0}
      <p
        class="rounded-md border bg-muted/60 px-3 py-2 text-sm text-muted-foreground"
        data-testid="today-hub-locked"
      >
        {m.today_hub_locked_notice({ count: data.lockedWorkspaceCount })}
      </p>
    {/if}

    {#if store.personalDiary && store.diaryStreak !== undefined}
      {@const diaryPlace = store.personalDiary}
      <div
        class="flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3"
        data-testid="today-hub-diary"
      >
        <span class="text-sm font-medium">
          {m.today_hub_diary_streak({ days: store.diaryStreak })}
        </span>
        <Button
          variant="outline"
          size="sm"
          class="roomy:min-h-touch"
          onclick={() => {
            router.go({ workspaceId: diaryPlace.workspaceId, diaryId: diaryPlace.diaryId });
          }}
        >
          {m.today_hub_diary_open()}
        </Button>
      </div>
    {/if}

    {#if store.status === 'loading' && !data}
      <p class="py-8 text-center text-muted-foreground" role="status">{m.app_loading()}</p>
    {:else if store.status === 'failed' && !data}
      <p class="py-8 text-center text-muted-foreground" role="alert">{m.app_problem_load()}</p>
    {:else if isEmpty}
      <p class="py-8 text-center text-muted-foreground" data-testid="today-hub-empty">
        {m.today_hub_empty()}
      </p>
    {:else if data}
      {#each [{ id: 'overdue', heading: m.today_hub_cards_overdue(), list: overdueCards }, { id: 'today', heading: m.today_hub_cards_today(), list: todayCards }, { id: 'soon', heading: m.today_hub_cards_soon(), list: soonCards }] as group (group.id)}
        {#if group.list.length > 0}
          <section class="grid gap-2" data-testid="today-hub-cards-{group.id}">
            <h3 class="text-sm font-semibold text-muted-foreground">{group.heading}</h3>
            <ul class="grid gap-1.5">
              {#each group.list as card (card.cardId)}
                <li>
                  <button
                    type="button"
                    class="flex w-full items-center justify-between gap-3 rounded-md border px-3 py-2 text-left hover:bg-accent/50 roomy:min-h-touch"
                    onclick={() => {
                      openCardItem(card);
                    }}
                  >
                    <span class="min-w-0 flex-1 truncate font-medium">
                      {card.icon ? `${card.icon} ` : ''}{card.title || m['KanbanCard.untitled']()}
                    </span>
                    <span class="shrink-0 text-xs text-muted-foreground">
                      {wsBadge(card.workspace)} · {card.boardTitle || m['Sidebar.untitled-board']()} ·
                      {card.dueDate}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      {/each}

      {#if data.units.length > 0}
        <section class="grid gap-2" data-testid="today-hub-units">
          <h3 class="text-sm font-semibold text-muted-foreground">{m.today_hub_learning()}</h3>
          <ul class="grid gap-1.5">
            {#each data.units as unit (unit.unitId)}
              <li>
                <button
                  type="button"
                  class="flex w-full items-center justify-between gap-3 rounded-md border px-3 py-2 text-left hover:bg-accent/50 roomy:min-h-touch"
                  onclick={() => {
                    openUnitItem(unit);
                  }}
                >
                  <span class="min-w-0 flex-1 truncate font-medium">
                    {unit.title || m.course_unit_untitled()}
                  </span>
                  <span class="shrink-0 text-xs text-muted-foreground">
                    {wsBadge(unit.workspace)} · {unit.courseTitle || m.course_untitled()} · {unit.effectiveDate}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if data.reviews.length > 0}
        <section class="grid gap-2" data-testid="today-hub-reviews">
          <h3 class="text-sm font-semibold text-muted-foreground">{m.today_hub_reviews()}</h3>
          <ul class="grid gap-1.5">
            {#each data.reviews as review (review.id)}
              <li>
                <button
                  type="button"
                  class="flex w-full items-center justify-between gap-3 rounded-md border px-3 py-2 text-left hover:bg-accent/50 roomy:min-h-touch"
                  onclick={() => {
                    openReviewItem(review);
                  }}
                >
                  <span class="min-w-0 flex-1 truncate font-medium">
                    {review.unitTitle || m.course_unit_untitled()}
                  </span>
                  <span class="shrink-0 text-xs text-muted-foreground">
                    {wsBadge(review.workspace)} · {review.courseTitle || m.course_untitled()} · {review.scheduledDate}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if data.meetings.length > 0}
        <section class="grid gap-2" data-testid="today-hub-meetings">
          <h3 class="text-sm font-semibold text-muted-foreground">{m.today_hub_meetings()}</h3>
          <ul class="grid gap-1.5">
            {#each data.meetings as meeting (meeting.meetingId)}
              <li>
                <button
                  type="button"
                  class="flex w-full items-center justify-between gap-3 rounded-md border px-3 py-2 text-left hover:bg-accent/50 roomy:min-h-touch"
                  onclick={() => {
                    openMeetingItem(meeting);
                  }}
                >
                  <span class="min-w-0 flex-1 truncate font-medium">{meeting.title}</span>
                  <span class="shrink-0 text-xs text-muted-foreground">
                    {wsBadge(meeting.workspace)} · {displayDateTime(meeting.startsAt, {
                      locale: workspace.locale,
                      timeZone: workspace.regional.timeZone,
                      hourCycle: workspace.regional.hourCycle,
                    })}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}
  </div>
</div>
