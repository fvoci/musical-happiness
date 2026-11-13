<script lang="ts">
  import AuditList from '$lib/components/audit/AuditList.svelte';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';
  import { AUDIT_ACTIONS, type AuditRecord } from '$lib/logic/audit.ts';

  import { STORY_NOW } from './fixtures.ts';

  let { count = AUDIT_ACTIONS.length, hidden = false }: { count?: number; hidden?: boolean } =
    $props();
  const person = {
    id: 'example-reader',
    username: 'example-reader',
    displayName: 'Example member · 예시 구성원',
  };
  const records = $derived<AuditRecord[]>(
    Array.from({ length: count }, (_, index) => ({
      id: `example-event-${index}`,
      action: AUDIT_ACTIONS[index % AUDIT_ACTIONS.length] ?? 'workspace_created',
      actorKind: index === AUDIT_ACTIONS.length - 1 ? 'server_operator' : 'person',
      actor:
        index === AUDIT_ACTIONS.length - 1
          ? null
          : hidden
            ? { ...person, username: '', displayName: '' }
            : person,
      subject: hidden ? null : { ...person, id: 'example-guest', username: 'example-guest' },
      targetId: 'example-workspace',
      targetName: hidden ? null : 'Example workspace with a long name · 이름이 긴 예시 작업 공간',
      createAt: STORY_NOW - index * 60_000,
    })),
  );
</script>

<div class="max-w-2xl">
  <AuditList {records} locale={getLocale()} timeZone="Asia/Seoul" />
</div>
