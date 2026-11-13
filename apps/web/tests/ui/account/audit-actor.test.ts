import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import AuditList from '$lib/components/audit/AuditList.svelte';
import type { AuditRecord } from '$lib/logic/audit.ts';

afterEach(cleanup);
it('distinguishes a server operator from a person whose account is gone', () => {
  const base = {
    action: 'sso_released',
    targetId: 'org',
    targetName: null,
    subject: null,
    createAt: 1,
  } as const;
  const records: AuditRecord[] = [
    { ...base, id: 'operator', actor: { kind: 'server_operator' } },
    {
      ...base,
      id: 'person',
      actor: { kind: 'person', id: 'former', username: '', displayName: '' },
    },
  ];
  render(AuditList, { records, locale: 'en' });
  expect(screen.getByTestId('audit-list')).toHaveTextContent(
    "Server operator released the organization's single sign-on requirement",
  );
  expect(screen.getByTestId('audit-list')).toHaveTextContent(
    "a former user released the organization's single sign-on requirement",
  );
});
