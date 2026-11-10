<!--
  Choosing the value of a date property: a month to pick a day from, the
  switch that turns the date into a range, and "clear" and "today". The same
  content in a bottom sheet and in a popover.

  A day is 32 pixels for a mouse and `--spacing-touch` for a finger; seven of
  those are 308 pixels, which fits a screen 360 wide with the padding around.
-->
<script lang="ts">
  import { CalendarDate, type DateValue } from '@internationalized/date';

  import { Button } from '$lib/ui/button/index.js';
  import { Calendar } from '$lib/ui/calendar/index.js';
  import { Switch } from '$lib/ui/switch/index.js';

  import { T } from './text.ts';

  let value = $state<DateValue | undefined>(new CalendarDate(2026, 11, 17));
  let range = $state(false);
</script>

<div class="flex flex-col">
  <Calendar
    type="single"
    bind:value
    locale="ko-KR"
    class="mx-auto bg-transparent [--cell-size:--spacing(8)] pointer-coarse:[--cell-size:var(--spacing-touch)]"
  />
  <label
    class="flex min-h-10 items-center justify-between gap-3 border-t px-4 pointer-coarse:min-h-12"
  >
    <span>{T.endDate}</span>
    <Switch bind:checked={range} />
  </label>
  <div class="flex items-center justify-between border-t p-2">
    <Button variant="ghost" class="pointer-coarse:h-touch pointer-coarse:px-3">{T.clearDate}</Button
    >
    <Button variant="outline" class="pointer-coarse:h-touch pointer-coarse:px-3">{T.today}</Button>
  </div>
</div>
