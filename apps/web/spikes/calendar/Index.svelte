<!--
  The chooser: the screens, and for the one picked its proposals side by side,
  each in a frame of the size picked. A frame is a page of its own, so the
  width its layout sees is the frame's (the pointer is still this device's).
-->
<script lang="ts">
  import { T as MOBILE } from '../mobile/text.ts';

  import {
    SCREENS,
    SIZES,
    addressOf,
    type ScreenId,
    type ScreenInfo,
    type Variant,
  } from './params.ts';
  import { T } from './text.ts';

  let picked = $state<ScreenId>('month');
  let dark = $state(false);
  let width = $state<keyof typeof SIZES>(360);

  const WIDTHS = [360, 626, 820, 890] as const;
  const frame = $derived(SIZES[width]);

  const screen = $derived<ScreenInfo>(SCREENS.find((entry) => entry.id === picked) ?? SCREENS[0]);
  const proposals = $derived(Object.entries(screen.variants) as [Variant, string][]);
  const extra = $derived(dark ? '&theme=dark' : '');

  function go(address: string): void {
    location.assign(address);
  }
</script>

<main class="mx-auto flex max-w-[120rem] flex-col gap-6 p-4 md:p-8">
  <header class="flex flex-col gap-2">
    <h1 class="text-2xl font-semibold">{MOBILE.logo} · {T.chooserTitle}</h1>
    <p class="max-w-prose text-base text-muted-foreground">{T.chooserLead}</p>
    <label class="flex min-h-9 items-center gap-2 self-start pointer-coarse:min-h-touch">
      <input type="checkbox" bind:checked={dark} class="size-4" />
      {MOBILE.chooserDark}
    </label>
    <div role="group" aria-label={MOBILE.chooserSize} class="flex flex-wrap items-center gap-2">
      <span class="text-muted-foreground">{MOBILE.chooserSize}</span>
      {#each WIDTHS as option (option)}
        <button
          type="button"
          aria-pressed={option === width}
          class="h-9 rounded-lg border px-3 tabular-nums hover:bg-muted aria-pressed:bg-muted aria-pressed:font-medium pointer-coarse:h-touch"
          onclick={() => {
            width = option;
          }}
        >
          {SIZES[option].width}×{SIZES[option].height}
        </button>
      {/each}
    </div>
  </header>

  <div class="flex flex-col gap-6 lg:flex-row lg:items-start">
    <nav aria-label={T.chooserTitle} class="flex shrink-0 flex-col gap-1 lg:w-72">
      {#each SCREENS as entry (entry.id)}
        <button
          type="button"
          aria-current={entry.id === picked ? 'true' : undefined}
          class="flex min-h-9 items-center rounded-lg px-3 text-left hover:bg-muted aria-[current]:bg-muted aria-[current]:font-medium pointer-coarse:min-h-touch"
          onclick={() => {
            picked = entry.id;
          }}
        >
          {entry.title}
        </button>
      {/each}
    </nav>

    <div class="flex min-w-0 flex-1 flex-wrap gap-6">
      {#each proposals as [variant, summary] (variant)}
        {@const label = screen.cases ? T.chooserCase(variant) : MOBILE.chooserVariant(variant)}
        <section class="flex max-w-full flex-col gap-2" style:width="{frame.width}px">
          <h2 class="font-semibold">{label}: {summary}</h2>
          <div class="flex flex-wrap gap-2">
            <button
              type="button"
              class="h-9 rounded-lg border px-3 hover:bg-muted pointer-coarse:h-touch"
              onclick={() => {
                go(addressOf(picked, variant, extra));
              }}
            >
              {MOBILE.chooserOpen}
            </button>
            <button
              type="button"
              class="h-9 rounded-lg border px-3 hover:bg-muted pointer-coarse:h-touch"
              onclick={() => {
                go(addressOf(picked, variant, `${extra}&open=0`));
              }}
            >
              {MOBILE.chooserClosed}
            </button>
          </div>
          <iframe
            title="{screen.title}, {label}"
            src={addressOf(picked, variant, extra)}
            class="hidden max-w-none rounded-xl border lg:block"
            style:width="{frame.width}px"
            style:height="{frame.height}px"
          ></iframe>
        </section>
      {/each}
    </div>
  </div>
</main>
