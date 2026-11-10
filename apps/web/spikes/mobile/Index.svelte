<!--
  The chooser: the screens, and for the one picked its proposals side by side,
  each in a frame the size of a phone. A frame is a page of its own, so the
  width its layout sees is the frame's (the pointer is still this device's).
-->
<script lang="ts">
  import { SCREENS, addressOf, type ScreenId, type Variant } from './params.ts';
  import { T } from './text.ts';

  let picked = $state<ScreenId>('sidebar');
  let dark = $state(false);

  const screen = $derived(SCREENS.find((entry) => entry.id === picked) ?? SCREENS[0]);
  const proposals = $derived(Object.entries(screen.variants) as [Variant, string][]);
  const extra = $derived(dark ? '&theme=dark' : '');

  function go(address: string): void {
    location.assign(address);
  }
</script>

<main class="mx-auto flex max-w-6xl flex-col gap-6 p-4 md:p-8">
  <header class="flex flex-col gap-2">
    <h1 class="text-2xl font-semibold">{T.logo} · {T.chooserTitle}</h1>
    <p class="max-w-prose text-base text-muted-foreground">{T.chooserLead}</p>
    <label class="flex min-h-touch items-center gap-2 self-start">
      <input type="checkbox" bind:checked={dark} class="size-4" />
      {T.chooserDark}
    </label>
  </header>

  <div class="flex flex-col gap-6 lg:flex-row lg:items-start">
    <nav aria-label={T.chooserTitle} class="flex shrink-0 flex-col gap-1 lg:w-72">
      {#each SCREENS as entry (entry.id)}
        <button
          type="button"
          aria-current={entry.id === picked ? 'true' : undefined}
          class="flex min-h-touch items-center rounded-lg px-3 text-left hover:bg-muted aria-[current]:bg-muted aria-[current]:font-medium"
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
        <section class="flex w-[360px] max-w-full flex-col gap-2">
          <h2 class="font-semibold">{T.chooserVariant(variant)}: {summary}</h2>
          <div class="flex flex-wrap gap-2">
            <button
              type="button"
              class="h-touch rounded-lg border px-3 hover:bg-muted"
              onclick={() => {
                go(addressOf(picked, variant, extra));
              }}
            >
              {T.chooserOpen}
            </button>
            <button
              type="button"
              class="h-touch rounded-lg border px-3 hover:bg-muted"
              onclick={() => {
                go(addressOf(picked, variant, `${extra}&open=0`));
              }}
            >
              {T.chooserClosed}
            </button>
          </div>
          <iframe
            title="{screen.title}, {T.chooserVariant(variant)}"
            src={addressOf(picked, variant, extra)}
            class="hidden h-[740px] w-[360px] rounded-xl border lg:block"
          ></iframe>
        </section>
      {/each}
    </div>
  </div>
</main>
