<!--
  Stand-ins for what a headless browser does not have: the notch and the home
  indicator of a phone (`?safe=1`) and its on-screen keyboard (`?keyboard=1`).
  They cover exactly the room that mobile.css reserves for them, so a
  screenshot shows whether anything is left underneath.
-->
<script lang="ts">
  import { CURRENT } from './current.ts';
  import { T } from './text.ts';

  const KEY_ROWS = [10, 9, 7];
</script>

{#if CURRENT.fakeSafeArea}
  <div
    aria-hidden="true"
    class="pointer-events-none fixed inset-x-0 top-0 z-[70] flex h-(--safe-top) items-start justify-center"
  >
    <div class="mt-2.5 h-7 w-28 rounded-full bg-foreground"></div>
  </div>
  {#if !CURRENT.fakeKeyboard}
    <div
      aria-hidden="true"
      class="pointer-events-none fixed inset-x-0 bottom-0 z-[70] flex h-(--safe-bottom) items-center justify-center"
    >
      <div class="h-1.25 w-32 rounded-full bg-foreground"></div>
    </div>
  {/if}
{/if}

{#if CURRENT.fakeKeyboard}
  <div
    aria-hidden="true"
    data-testid="fake-keyboard"
    class="pointer-events-none fixed inset-x-0 bottom-0 z-[70] flex h-(--keyboard) flex-col gap-2 border-t bg-muted px-1 pt-2"
  >
    {#each KEY_ROWS as keys (keys)}
      <div class="flex justify-center gap-1.5">
        <!-- eslint-disable-next-line @typescript-eslint/no-unused-vars -->
        {#each { length: keys } as _, at (at)}
          <div class="h-10 w-7.5 rounded-md bg-background shadow-xs"></div>
        {/each}
      </div>
    {/each}
    <div class="flex justify-center gap-1.5">
      <div class="h-10 w-12 rounded-md bg-background shadow-xs"></div>
      <div
        class="flex h-10 flex-1 items-center justify-center rounded-md bg-background text-xs text-muted-foreground shadow-xs"
      >
        {T.fakeKeyboard}
      </div>
      <div class="h-10 w-20 rounded-md bg-primary shadow-xs"></div>
    </div>
  </div>
{/if}
