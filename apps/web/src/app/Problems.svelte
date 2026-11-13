<!--
  Tells the reader what went wrong: a change that was refused and taken back,
  a file that did not upload. One line each, in a live region, until
  dismissed or a while has passed.
-->
<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { humanFileSize } from '$lib/logic/format.ts';
  import type { Problem, Workspace } from '$lib/state/index.ts';

  let { workspace }: { workspace: Workspace } = $props();

  const workspaceName = $derived(workspace.info?.title || m.shell_workspace_unnamed());

  /** How long a line stays when nobody dismisses it, in milliseconds. */
  const STAY = 8000;

  function text(problem: Problem): string {
    const error = problem.error;
    // No answer at all.
    if (error?.status === 0) return m.app_problem_network();
    if (problem.action === 'import' && error?.status === 403) {
      return m.app_problem_import_forbidden({ workspace: workspaceName });
    }
    if (error?.status === 403) return m.app_problem_forbidden();
    if (problem.action === 'upload' && error?.status === 429) {
      return m.app_problem_upload_busy();
    }
    if (problem.action === 'upload' && error?.status === 413 && error.limit) {
      return m.app_problem_upload_too_large({ limit: humanFileSize(error.limit) });
    }
    if (problem.action === 'import' && error?.status === 413) {
      return error.reason === 'over_import_limit' && error.limit
        ? m.app_problem_import_too_large({ limit: humanFileSize(error.limit) })
        : m.app_problem_import_too_big();
    }
    if (problem.action === 'import' && error?.status === 400) {
      return m.app_problem_import_invalid();
    }
    if (problem.action === 'import' && error?.status === 429) {
      return m.app_problem_upload_busy();
    }
    switch (problem.action) {
      case 'export':
        return m.app_problem_export();
      case 'import':
        return m.app_problem_import();
      case 'load':
        return m.app_problem_load();
      case 'undo':
        return m.app_problem_undo();
      case 'upload':
        return m.app_problem_upload();
      default:
        return m.app_problem_save();
    }
  }

  $effect(() => {
    const timers = workspace.problems.map((problem) =>
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
