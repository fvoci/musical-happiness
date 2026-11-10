<!-- The chooser, or the mock-up the address names. -->
<script lang="ts">
  import { CURRENT } from './current.ts';
  import DeviceChrome from './DeviceChrome.svelte';
  import Index from './Index.svelte';
  import Shell from './Shell.svelte';
</script>

{#if CURRENT.screen}
  <Shell />
  <DeviceChrome />
{:else}
  <Index />
{/if}
