/**
 * A workspace of an organization that takes no password: what the reader's
 * session is told (401, `sso_required`) is not the session being over, and
 * the workspace is shown closed, with where to sign in. And the requests that
 * connect a single sign-on to the account.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
import { workspacesOf } from '$lib/api/convert.ts';
import { BackendError, isSignedOut, ssoOrgOf, type WorkspaceInfo } from '$lib/state/index.ts';

import { setup, type Setup } from './support.ts';

const ORG = 'the-organization';

const ssoRequired = (): BackendError =>
  new BackendError(401, { reason: 'sso_required', type: '/problems/sso-required', orgId: ORG });

afterEach(() => {
  vi.unstubAllGlobals();
});

/** The world's workspace as the server lists it for an organization, closed or not. */
function listed(made: Setup, closed: boolean): WorkspaceInfo {
  const workspace = { ...made.world.workspace, orgId: ORG, ssoRequired: closed };
  vi.spyOn(made.backend, 'workspaces').mockResolvedValue([workspace]);
  return workspace;
}

describe('a refusal that asks for single sign-on', () => {
  it('names the organization, and is not the session being over', () => {
    expect(ssoOrgOf(ssoRequired())).toBe(ORG);
    expect(isSignedOut(ssoRequired())).toBe(false);
    const over = new BackendError(401, { detail: 'no session token' });
    expect(ssoOrgOf(over)).toBeUndefined();
    expect(isSignedOut(over)).toBe(true);
    expect(isSignedOut(new BackendError(403))).toBe(false);
    expect(ssoOrgOf(new Error('anything else'))).toBeUndefined();
  });

  it('comes from the server with its organization', async () => {
    vi.stubGlobal('fetch', () =>
      Promise.resolve(
        new Response(
          JSON.stringify({ status: 401, reason: 'sso_required', orgId: ORG, detail: 'sign in' }),
          { status: 401, headers: { 'Content-Type': 'application/problem+json' } },
        ),
      ),
    );
    const error: unknown = await new ServerBackend('')
      .boards('w')
      .catch((caught: unknown) => caught);
    expect(ssoOrgOf(error)).toBe(ORG);
  });
});

describe('the workspaces as the server lists them', () => {
  it('say which organization each is of, and which are closed to this session', () => {
    const workspaces = workspacesOf([
      { id: 'a', orgId: ORG, name: 'A', role: 'member', ssoRequired: true },
      { id: 'b', orgId: 'another', name: 'B', role: 'admin', ssoRequired: false },
    ]);
    expect(workspaces).toEqual([
      { id: 'a', title: 'A', orgId: ORG, ssoRequired: true },
      { id: 'b', title: 'B', orgId: 'another', ssoRequired: false },
    ]);
  });
});

describe('a workspace closed to the session', () => {
  it('is entered as closed, by its name, and nothing of it is read', async () => {
    const made = setup();
    const closed = listed(made, true);
    const boards = vi.spyOn(made.backend, 'boards');
    await made.workspace.load();
    expect(made.workspace.status).toBe('ready');
    expect(made.workspace.entered).toBe('sso');
    expect(made.workspace.info).toEqual(closed);
    expect(made.workspace.boards).toEqual([]);
    expect(boards).not.toHaveBeenCalled();
  });

  it('is closed when reading it says so, though the list did not', async () => {
    const made = setup();
    const open = listed(made, false);
    vi.spyOn(made.backend, 'boards').mockRejectedValue(ssoRequired());
    await made.workspace.load();
    expect(made.workspace.status).toBe('ready');
    expect(made.workspace.entered).toBe('sso');
    expect(made.workspace.info).toEqual({ ...open, ssoRequired: true });
  });

  it('closes under the reader when a later request says so, and the session goes on', async () => {
    const made = setup();
    listed(made, false);
    await made.workspace.load();
    expect(made.workspace.entered).toBe('ready');
    expect(made.workspace.boards.length).toBeGreaterThan(0);

    made.workspace.report('save', ssoRequired());
    expect(made.workspace.status).toBe('ready');
    expect(made.workspace.entered).toBe('sso');
    expect(made.workspace.info?.orgId).toBe(ORG);
    expect(made.workspace.boards).toEqual([]);
    expect(made.workspace.problems).toEqual([]);
    expect(made.workspace.workspaces.map((one) => one.ssoRequired)).toEqual([true]);

    // A session that is over is still that.
    made.workspace.report('save', new BackendError(401));
    expect(made.workspace.status).toBe('signedOut');
  });
});

describe('connecting a single sign-on to the account', () => {
  interface Sent {
    url: string;
    method: string;
    body: unknown;
  }

  function serve(answer: unknown, status = 200): Sent[] {
    const sent: Sent[] = [];
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      sent.push({
        url,
        method: init.method ?? 'GET',
        body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
      });
      return Promise.resolve(
        new Response(status === 204 ? null : JSON.stringify(answer), {
          status,
          headers: { 'Content-Type': 'application/json' },
        }),
      );
    });
    return sent;
  }

  it('starts a sign-in with no body, and one that connects with the password', async () => {
    const sent = serve({ authorizationUrl: 'https://idp.example.com/authorize' });
    const backend = new ServerBackend('');
    expect(await backend.startOidc(ORG)).toBe('https://idp.example.com/authorize');
    await backend.startOidc(ORG, 'the password');
    expect(sent).toEqual([
      { url: `/api/v1/auth/oidc/start?org=${ORG}`, method: 'POST', body: undefined },
      {
        url: `/api/v1/auth/oidc/start?org=${ORG}`,
        method: 'POST',
        body: { linkPassword: 'the password' },
      },
    ]);
  });

  it('reads what is connected, and disconnects one with the password', async () => {
    const identity = {
      issuer: 'https://idp.example.com',
      subject: 'there',
      email: 'ada@example.com',
      createAt: 5,
    };
    const read = serve([identity]);
    const backend = new ServerBackend('');
    expect(await backend.identities()).toEqual([identity]);
    expect(read[0]?.url).toBe('/api/v1/users/me/identities');

    const sent = serve(undefined, 204);
    await backend.unlinkIdentity(identity, 'the password');
    expect(sent).toEqual([
      {
        url: '/api/v1/users/me/identities',
        method: 'DELETE',
        body: { issuer: identity.issuer, subject: 'there', password: 'the password' },
      },
    ]);
  });
});
