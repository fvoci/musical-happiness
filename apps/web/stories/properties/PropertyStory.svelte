<!--
  The stand a property is shown on in Storybook. It plays the caller: it
  keeps the stored value and the options, applies what the component reports,
  and writes every report on the screen. So a value really changes when it is
  edited here, with a keyboard or a finger, and what was stored can be read
  next to it.

  The same value is drawn twice: as it is edited (a row of a card's
  properties) and as it is only shown (a table cell, a badge on a card).
-->
<script lang="ts" module>
  import type { HourCycle } from '$lib/logic/format.ts';
  import type { Weekday } from '$lib/logic/regional.ts';
  import type { PropertyType, PropertyValue as StoredValue } from '$lib/logic/wire.ts';

  export interface PropertyStoryProps {
    type: PropertyType;
    /** The value the story starts with. */
    value?: StoredValue;
    readOnly?: boolean;
    startOpen?: boolean;
    /** `auto` follows the width of the viewport, like the app will. */
    presentation?: 'auto' | 'popover' | 'sheet';
    /** The time zone of the reader's account: not the one of this device. */
    timeZone?: string;
    hourCycle?: HourCycle;
    /** 1 is Monday, 7 is Sunday. */
    weekStart?: Weekday;
    /** How many options a select property has. */
    optionCount?: number;
    nameFormat?: string;
  }
</script>

<script lang="ts">
  import { DEFAULT_OPTION_COLOR, OPTION_COLORS } from '$lib/components/option-chip/colors.ts';
  import { PickerPresentation } from '$lib/components/picker-surface/default-presentation.svelte.ts';
  import PropertyValue from '$lib/components/properties/PropertyValue.svelte';
  import type { PropertyUser } from '$lib/components/properties/types.ts';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { createCard, createId, type PropertyOption } from '$lib/logic/wire.ts';

  let {
    type,
    value,
    readOnly = false,
    startOpen = false,
    presentation = 'auto',
    timeZone = 'Asia/Seoul',
    hourCycle = 'h12',
    weekStart = 7,
    optionCount = 4,
    nameFormat = 'nickname_full_name',
  }: PropertyStoryProps = $props();

  const NAMES: Partial<Record<PropertyType, string>> = {
    text: '메모 Note',
    number: '예상 시간 Estimate',
    email: '담당자 메일 Email',
    phone: '연락처 Phone',
    url: '참고 링크 Link',
    select: '상태 Status',
    multiSelect: '태그 Tags',
    date: '마감 Due',
    person: '담당자 Assignee',
    multiPerson: '검토자 Reviewers',
    checkbox: '완료 Done',
    createdTime: '만든 시각 Created',
    createdBy: '만든 사람 Created by',
    updatedTime: '고친 시각 Updated',
    updatedBy: '고친 사람 Updated by',
  };

  const users: PropertyUser[] = [
    { id: 'u-jisoo', username: 'jisoo', nickname: '', firstname: '지수', lastname: '김' },
    { id: 'u-minho', username: 'minho', nickname: '민호', firstname: 'Minho', lastname: 'Lee' },
    { id: 'u-ada', username: 'ada', nickname: '', firstname: 'Ada', lastname: 'Lovelace' },
    { id: 'u-grace', username: 'grace', nickname: '', firstname: 'Grace', lastname: 'Hopper' },
    { id: 'u-haru', username: 'haru', nickname: '', firstname: '하루', lastname: '박' },
    { id: 'u-bot', username: 'deploy-bot', nickname: '', firstname: '', lastname: '' },
  ];

  const BASE_OPTIONS = ['할 일 To do', '진행 중 In progress', '검토 Review', '완료 Done'];
  // svelte-ignore state_referenced_locally (the story starts with these options and then owns them)
  let options = $state<PropertyOption[]>(
    Array.from({ length: optionCount }, (_, index) => ({
      id: `option-${index + 1}`,
      value: BASE_OPTIONS[index] ?? `선택지 ${index + 1}`,
      color: OPTION_COLORS[index % OPTION_COLORS.length] ?? DEFAULT_OPTION_COLOR,
    })),
  );

  // svelte-ignore state_referenced_locally (the story starts with this value and then owns it)
  let stored = $state.raw<StoredValue | undefined>(value);
  let log = $state<string[]>([]);

  const card = createCard();
  card.createdBy = 'u-jisoo';
  card.modifiedBy = 'u-minho';
  // Half past nine in the evening of 9 November 2026 in Seoul, and 01:05 the next night.
  card.createAt = Date.UTC(2026, 10, 9, 12, 30);
  card.updateAt = Date.UTC(2026, 10, 10, 16, 5);

  const template = $derived({ id: 'property', name: NAMES[type] ?? type, type, options });
  const regional = $derived({ timeZone, weekStart, hourCycle });
  const viewport = new PickerPresentation();
  const surface = $derived(presentation === 'auto' ? viewport.current : presentation);
  const locale = getLocale();

  const show = (detail: unknown): string =>
    detail === undefined ? 'undefined' : JSON.stringify(detail);

  function report(what: string, detail: unknown): void {
    log = [`${what} ${show(detail)}`, ...log].slice(0, 8);
  }

  function replaceOption(id: string, change: Partial<PropertyOption>): void {
    options = options.map((option) => (option.id === id ? { ...option, ...change } : option));
  }
</script>

<div class="grid max-w-xl gap-6">
  <section class="grid gap-2">
    <h2 class="text-xs font-medium text-muted-foreground">편집 Edit</h2>
    <div class="grid grid-cols-[9rem_minmax(0,1fr)] items-start gap-2">
      <div class="py-1.5 text-muted-foreground">{template.name}</div>
      <PropertyValue
        {template}
        {card}
        value={stored}
        {readOnly}
        {startOpen}
        showEmptyPlaceholder
        {users}
        {nameFormat}
        {locale}
        {regional}
        presentation={surface}
        onChange={(next: StoredValue | undefined) => {
          stored = next;
          report('onChange', next);
        }}
        onCreateOption={(name: string) => {
          const option = { id: createId('block'), value: name, color: DEFAULT_OPTION_COLOR };
          options = [...options, option];
          report('onCreateOption', name);
          return option;
        }}
        onRenameOption={(option: PropertyOption, name: string) => {
          replaceOption(option.id, { value: name });
          report('onRenameOption', [option.value, name]);
        }}
        onRecolorOption={(option: PropertyOption, color: string) => {
          replaceOption(option.id, { color });
          report('onRecolorOption', [option.value, color]);
        }}
        onDeleteOption={(option: PropertyOption) => {
          options = options.filter((other) => other.id !== option.id);
          report('onDeleteOption', option.value);
        }}
      />
    </div>
  </section>

  <section class="grid gap-2">
    <h2 class="text-xs font-medium text-muted-foreground">표시 Display</h2>
    <div class="w-56 rounded-md border">
      <PropertyValue
        {template}
        {card}
        value={stored}
        mode="display"
        {users}
        {nameFormat}
        {locale}
        {regional}
      />
    </div>
  </section>

  <section class="grid gap-1 text-xs text-muted-foreground">
    <div>
      저장된 값 Stored value · 계정 시간대 Account time zone: {timeZone} · {surface}
    </div>
    <pre
      class="overflow-x-auto rounded-md bg-muted p-2 font-mono text-foreground"
      data-testid="stored">{show(stored)}</pre>
    <ol class="m-0 grid list-none gap-0.5 p-0 font-mono" aria-label="Callbacks" data-testid="log">
      {#each log as line, index (log.length - index)}
        <li class="truncate">{line}</li>
      {/each}
    </ol>
  </section>
</div>
