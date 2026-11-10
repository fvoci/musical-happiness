<!-- The chooser, or the mock-up the address names. -->
<script lang="ts">
  import DeviceChrome from '../mobile/DeviceChrome.svelte';

  import { CURRENT } from './current.ts';
  import Index from './Index.svelte';
  import Shell from './Shell.svelte';
</script>

{#if CURRENT.screen}
  <Shell />
  <DeviceChrome />
{:else}
  <Index />
{/if}
