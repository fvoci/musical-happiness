<!--
  Tells the reader what went wrong: a change that was refused and taken back,
  a file that did not upload, a write that met somebody else's. One line each,
  in a live region, until dismissed (or, for all but a conflict, a while has
  passed).
-->
<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { humanFileSize } from '$lib/logic/format.ts';
  import type { Problem, Workspace } from '$lib/state/index.ts';

  let { workspace }: { workspace: Workspace } = $props();

  /** How long a line stays when nobody dismisses it, in milliseconds. */
  const STAY = 8000;

  function text(problem: Problem): string {
    const error = problem.error;
    if (error?.kind === 'network') return m.app_problem_network();
    if (error?.kind === 'forbidden') return m.app_problem_forbidden();
    // What the server does not take yet (`docs/decisions.md`, 연결 3): said as that, not as a refusal.
    if (error?.reason === 'not_wired') return m.app_problem_not_wired();
    if (problem.action === 'upload' && error?.kind === 'rate_limited') {
      return m.app_problem_upload_busy();
    }
    if (problem.action === 'upload' && error?.kind === 'payload_too_large' && error.limit) {
      return m.app_problem_upload_too_large({ limit: humanFileSize(error.limit) });
    }
    switch (problem.action) {
      case 'load':
        return m.app_problem_load();
      case 'undo':
        return m.app_problem_undo();
      case 'upload':
        return m.app_problem_upload();
      case 'conflict':
        return m.app_problem_conflict();
      default:
        return m.app_problem_save();
    }
  }

  $effect(() => {
    const timers = workspace.problems
      .filter((problem) => problem.action !== 'conflict')
      .map((problem) =>
        setTimeout(() => {
          workspace.dismissProblem(problem.id);
        }, STAY),
      );
    return () => {
      for (const timer of timers) clearTimeout(timer);
    };
  });
</script>

<div
  class="pointer-events-none fixed inset-x-0 bottom-[calc(var(--safe-bottom)+var(--keyboard)+0.75rem)] z-[60] flex flex-col items-center gap-2 pr-[calc(var(--safe-right)+0.75rem)] pl-[calc(var(--safe-left)+0.75rem)]"
  role="status"
  aria-live="polite"
  data-testid="problems"
>
  {#each workspace.problems as problem (problem.id)}
    <div
      class="pointer-events-auto flex max-w-md items-center gap-2 rounded-lg border bg-popover py-2 pr-2 pl-3 text-popover-foreground shadow-md"
      data-problem={problem.action}
    >
      <p class="min-w-0 flex-1">{text(problem)}</p>
      <button
        type="button"
        class="grid size-8 shrink-0 place-items-center rounded-md hover:bg-muted roomy:size-touch"
        aria-label={m.app_problem_dismiss()}
        onclick={() => {
          workspace.dismissProblem(problem.id);
        }}
      >
        <XIcon class="size-4" aria-hidden="true" />
      </button>
    </div>
  {/each}
</div>
