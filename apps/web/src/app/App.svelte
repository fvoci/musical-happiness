<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
</script>

<main class="grid min-h-dvh place-items-center p-4">
  <div class="text-center">
    <p class="text-2xl font-semibold tracking-tight" translate="no">{m.app_name()}</p>
    <p class="mt-2 text-muted-foreground" role="status">{m.app_loading()}</p>
  </div>
</main>
