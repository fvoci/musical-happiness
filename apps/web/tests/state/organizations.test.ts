/**
 * The organization's administration against a stand-in for `fetch`: which
 * organization the reader administers, what each change sends, and what the
 * organization screen makes of a refusal.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { Http } from '$lib/api/http.ts';
import { ServerOrganizations } from '$lib/api/organizations.ts';
import { OrgAdmin, refusalOf } from '$lib/state/org-admin.svelte.ts';
import { BackendError } from '$lib/state/index.ts';

interface Sent {
  url: string;
  method: string;
  body: unknown;
}

/** Answers each request by its address, and keeps what was sent. */
function serve(answers: Record<string, { status?: number; body?: unknown }>): Sent[] {
  const sent: Sent[] = [];
  vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
    const method = init.method ?? 'GET';
    sent.push({
      url,
      method,
      body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
    });
    const answer = answers[`${method} ${url.split('?')[0]}`] ?? { status: 404, body: {} };
    const status = answer.status ?? 200;
    return Promise.resolve(
      new Response(status === 204 ? null : JSON.stringify(answer.body ?? {}), {
        status,
        headers: {
          'Content-Type': status >= 400 ? 'application/problem+json' : 'application/json',
        },
      }),
    );
  });
  return sent;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

const organizations = (): ServerOrganizations => new ServerOrganizations(new Http(''));

describe('the organization on the server', () => {
  it('finds the organization the reader administers, and none where every one is not there', async () => {
    serve({
      'GET /api/v1/users/me/workspaces': {
        body: [
          { id: 'w1', orgId: 'o1', name: '', role: 'member' },
          { id: 'w2', orgId: 'o2', name: '', role: 'admin' },
        ],
      },
      'GET /api/v1/org/o2': {
        body: { id: 'o2', name: '', publicLinks: true, defaultWorkspaceId: 'w2' },
      },
    });
    expect(await organizations().administered()).toBe('o2');

    serve({ 'GET /api/v1/users/me/workspaces': { body: [{ id: 'w1', orgId: 'o1' }] } });
    expect(await organizations().administered()).toBeUndefined();
  });

  it('passes over an organization the session does not open, for the next it administers', async () => {
    const closed = { status: 401, body: { status: 401, reason: 'sso_required', orgId: 'o1' } };
    const mine = [
      { id: 'w1', orgId: 'o1', name: '', role: 'admin', ssoRequired: true },
      { id: 'w2', orgId: 'o2', name: '', role: 'admin' },
    ];
    serve({
      'GET /api/v1/users/me/workspaces': { body: mine },
      'GET /api/v1/org/o1': closed,
      'GET /api/v1/org/o2': { body: { id: 'o2', name: '', publicLinks: true } },
    });
    expect(await organizations().administered()).toBe('o2');

    serve({
      'GET /api/v1/users/me/workspaces': { body: mine.slice(0, 1) },
      'GET /api/v1/org/o1': closed,
    });
    expect(await organizations().administered()).toBeUndefined();

    // A session that is over is no organization passed over: it is told.
    serve({
      'GET /api/v1/users/me/workspaces': { body: mine },
      'GET /api/v1/org/o1': { status: 401, body: { status: 401 } },
    });
    await expect(organizations().administered()).rejects.toMatchObject({ status: 401 });
  });

  it('sends each change to its route', async () => {
    const sent = serve({
      'POST /api/v1/org/o/workspaces': { status: 201, body: {} },
      'PATCH /api/v1/org/o/workspaces/w': { body: {} },
      'POST /api/v1/org/o/workspaces/w/join': { body: {} },
      'PUT /api/v1/org/o/members/u/role': { status: 204 },
      'PATCH /api/v1/org/o': { body: {} },
      'GET /api/v1/w/w/audit': { body: [] },
    });
    const api = organizations();
    await api.createWorkspace('o', 'Design');
    await api.renameWorkspace('o', 'w', 'Product');
    await api.joinWorkspace('o', 'w');
    await api.setRole('o', 'u', 'member');
    await api.changeOrganization('o', { publicLinks: false });
    await api.workspaceRecords('w');
    expect(sent.map(({ method, url, body }) => [method, url, body])).toEqual([
      ['POST', '/api/v1/org/o/workspaces', { name: 'Design' }],
      ['PATCH', '/api/v1/org/o/workspaces/w', { name: 'Product' }],
      ['POST', '/api/v1/org/o/workspaces/w/join', undefined],
      ['PUT', '/api/v1/org/o/members/u/role', { role: 'member' }],
      ['PATCH', '/api/v1/org/o', { publicLinks: false }],
      ['GET', '/api/v1/w/w/audit?page=0&perPage=200', undefined],
    ]);
  });
});

describe('the organization screen', () => {
  it('names the refusals it tells apart', () => {
    const refused = (status: 404 | 409, reason?: string): BackendError =>
      new BackendError(status, reason ? { reason } : {});
    expect(refusalOf(refused(409, 'workspace_name_taken'))).toBe('name_taken');
    expect(refusalOf(refused(409, 'default_workspace'))).toBe('default_workspace');
    expect(refusalOf(refused(409, 'last_org_admin'))).toBe('last_org_admin');
    expect(refusalOf(refused(404))).toBe('not_found');
    expect(refusalOf(new Error('no'))).toBe('failed');
  });

  it('keeps a refusal to tell, and reads the lists again', async () => {
    const empty = { body: [] };
    serve({
      'GET /api/v1/org/o': { body: { id: 'o', name: 'Acme', publicLinks: true } },
      'GET /api/v1/org/o/workspaces': empty,
      'GET /api/v1/org/o/members': empty,
      'GET /api/v1/org/o/audit': empty,
      'POST /api/v1/org/o/workspaces': {
        status: 409,
        body: { status: 409, reason: 'workspace_name_taken' },
      },
    });
    const admin = new OrgAdmin(organizations(), 'o');
    await admin.load();
    expect(admin.status).toBe('ready');
    expect(await admin.createWorkspace('Taken')).toBe(false);
    expect(admin.refusal).toBe('name_taken');
    expect(admin.organization?.name).toBe('Acme');
  });

  it('asks for single sign-on where the organization takes no other session, at once or after a change', async () => {
    const closed = { status: 401, body: { status: 401, reason: 'sso_required', orgId: 'o' } };
    const answers: Record<string, { status?: number; body?: unknown }> = {
      'GET /api/v1/org/o': closed,
      'GET /api/v1/org/o/workspaces': closed,
      'GET /api/v1/org/o/members': closed,
      'GET /api/v1/org/o/audit': closed,
    };
    serve(answers);
    const admin = new OrgAdmin(organizations(), 'o');
    await admin.load();
    expect(admin.status).toBe('sso');

    // Closed while the screen was open: the change is refused, and the screen says why.
    const empty = { body: [] };
    serve({
      'GET /api/v1/org/o': { body: { id: 'o', name: 'Acme', publicLinks: true } },
      'GET /api/v1/org/o/workspaces': empty,
      'GET /api/v1/org/o/members': empty,
      'GET /api/v1/org/o/audit': empty,
    });
    const open = new OrgAdmin(organizations(), 'o');
    await open.load();
    expect(open.status).toBe('ready');
    serve({ ...answers, 'PATCH /api/v1/org/o': closed });
    expect(await open.setPublicLinks(false)).toBe(false);
    expect(open.status).toBe('sso');
  });

  it('passes on a session that ended with a change, and not one the organization closed', async () => {
    const ended = { status: 401, body: { status: 401, reason: 'unauthorized' } };
    serve({
      'GET /api/v1/org/o': ended,
      'GET /api/v1/org/o/workspaces': ended,
      'GET /api/v1/org/o/members': ended,
      'GET /api/v1/org/o/audit': ended,
    });
    const signedOut = vi.fn();
    const admin = new OrgAdmin(organizations(), 'o', { report: signedOut });
    await admin.load();
    expect(admin.status).toBe('failed');
    expect(signedOut).toHaveBeenCalledExactlyOnceWith('load', expect.any(BackendError));

    const closed = { status: 401, body: { status: 401, reason: 'sso_required', orgId: 'o' } };
    serve({
      'GET /api/v1/org/o': closed,
      'GET /api/v1/org/o/workspaces': closed,
      'GET /api/v1/org/o/members': closed,
      'GET /api/v1/org/o/audit': closed,
    });
    const notice = vi.fn();
    const walled = new OrgAdmin(organizations(), 'o', { report: notice });
    await walled.load();
    expect(walled.status).toBe('sso');
    expect(notice).not.toHaveBeenCalled();
  });

  it('is missing for whoever does not administer the organization', async () => {
    serve({});
    const admin = new OrgAdmin(organizations(), 'o');
    await admin.load();
    expect(admin.status).toBe('missing');
  });
});
