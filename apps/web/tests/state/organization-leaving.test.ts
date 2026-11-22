/**
 * Leaving an organization from the settings: the reader's workspaces are read
 * again, which have none of it; which organizations the reader can leave, by
 * the names they know them by; and what a refusal is told as, by its reason.
 */
import { describe, expect, it, vi } from 'vitest';

import {
  leavableOrganizations,
  leaveProblemOf,
} from '$lib/features/account/organization-leaving.ts';
import {
  BackendError,
  leaveOrganization,
  type Backend,
  type WorkspaceInfo,
} from '$lib/state/index.ts';

const own: WorkspaceInfo = { id: 'own', title: '', kind: 'personal', role: 'admin', orgId: 'me' };
const design: WorkspaceInfo = {
  id: 'd',
  title: 'Design',
  kind: 'team',
  role: 'member',
  orgId: 'a',
};
const sales: WorkspaceInfo = { id: 's', title: 'Sales', kind: 'team', role: 'admin', orgId: 'a' };
const visited: WorkspaceInfo = {
  id: 'v',
  title: 'Visited',
  kind: 'team',
  role: 'guest',
  orgId: 'b',
};
const plain: WorkspaceInfo = { id: 'p', title: '', kind: 'team', role: 'member', orgId: 'c' };

function holder(read: () => Promise<WorkspaceInfo[]>) {
  return {
    workspaces: [own, design, sales, plain],
    backend: { workspaces: read } as unknown as Backend,
  };
}

describe('leaving an organization', () => {
  it('reads the reader’s workspaces again once it is left', async () => {
    const shown = holder(() => Promise.resolve([own, plain]));
    const leave = vi.fn(() => Promise.resolve());
    await leaveOrganization(shown, leave, 'a');
    expect(leave).toHaveBeenCalledExactlyOnceWith('a');
    expect(shown.workspaces).toEqual([own, plain]);
  });

  it('takes the organization’s workspaces out when the list cannot be read', async () => {
    const shown = holder(() => Promise.reject(new BackendError(0)));
    await leaveOrganization(shown, () => Promise.resolve(), 'a');
    expect(shown.workspaces).toEqual([own, plain]);
  });

  it('changes nothing when it is refused', async () => {
    const read = vi.fn(() => Promise.resolve([own]));
    const shown = holder(read);
    const refusal = new BackendError(409, { reason: 'sole_org_admin' });
    await expect(leaveOrganization(shown, () => Promise.reject(refusal), 'a')).rejects.toBe(
      refusal,
    );
    expect(read).not.toHaveBeenCalled();
    expect(shown.workspaces).toEqual([own, design, sales, plain]);
  });
});

describe('the organizations the reader can leave', () => {
  it('are each once, by the names of their workspaces, and not one they are a guest of', () => {
    expect(leavableOrganizations([own, design, visited, sales, plain])).toEqual([
      { orgId: 'a', name: 'Design, Sales' },
      { orgId: 'c', name: 'Unnamed organization' },
    ]);
  });
});

describe('what a refusal is told as', () => {
  it('says what to do first, and that asking again changes nothing until then', () => {
    const told = (reason: string) => leaveProblemOf(new BackendError(409, { reason }));
    expect(told('sole_org_admin')).toEqual({
      text: expect.stringContaining('only administrator') as string,
      settled: true,
    });
    expect(told('last_instance_admin').text).toContain('only administrator of this server');
    expect(told('alone_in_organization').text).toContain('Invite someone first');
    expect(told('alone_in_organization').settled).toBe(true);
  });

  it('tells one that is no member any more, and lets anything else be tried again', () => {
    expect(leaveProblemOf(new BackendError(404)).settled).toBe(true);
    expect(leaveProblemOf(new BackendError(0))).toEqual({
      text: 'That did not work. Try again.',
      settled: false,
    });
    expect(leaveProblemOf(new Error('offline')).settled).toBe(false);
  });
});
