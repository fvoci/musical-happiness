<!--
  The bar above an open course the reader is no member of: they read it as it
  is, and join it here with the course's minimum role.
-->
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import type { MemberRole } from '$lib/logic/wire.ts';
  import { Button } from '$lib/ui/button/index.js';

  interface Props {
    joining: boolean;
    role: MemberRole | undefined;
    onJoin: () => void;
  }

  let { joining, role, onJoin }: Props = $props();

  const uid = $props.id();
  const ROLE_LABELS = {
    commenter: () => m['BoardMember.roleCommenter'](),
    viewer: () => m['BoardMember.roleViewer'](),
  };
  const hint = $derived(
    role === 'commenter' || role === 'viewer'
      ? m.course_join_hint_as({ role: ROLE_LABELS[role]() })
      : m.course_join_hint(),
  );
</script>

<div
  class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-b bg-muted px-3 py-2"
  data-testid="course-join"
>
  <p id="{uid}-hint" class="min-w-0 flex-1 text-sm text-muted-foreground">{hint}</p>
  <Button
    size="sm"
    class="roomy:min-h-touch"
    aria-describedby="{uid}-hint"
    aria-disabled={joining}
    aria-busy={joining}
    onclick={() => {
      if (!joining) onJoin();
    }}>{m.course_join()}</Button
  >
</div>
