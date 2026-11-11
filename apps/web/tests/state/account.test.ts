/**
 * The reader's account beside their settings: the display name, as the
 * memory backend keeps it and the server backend sends it, and as the
 * workspace shows it.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ServerBackend } from '$lib/api/backend.ts';
import { BackendError } from '$lib/state/index.ts';

import { setup } from './support.ts';

const rejection = async (promise: Promise<unknown>): Promise<BackendError> => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof BackendError) return error;
    throw error;
  }
  throw new Error('the call did not reject');
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the display name, in memory', () => {
  it('is kept without the white space around it, and empty or null is none', async () => {
    const { backend } = setup();
    expect((await backend.patchMe({ displayName: '  Ada  ' })).displayName).toBe('Ada');
    expect((await backend.me()).displayName).toBe('Ada');
    expect((await backend.patchMe({ displayName: null })).displayName).toBe('');
    expect((await backend.patchMe({ displayName: ' ' })).displayName).toBe('');
  });

  it('refuses what the server refuses, at the member, and changes nothing', async () => {
    const { backend } = setup();
    await backend.patchMe({ displayName: 'Ada' });
    const zeroWidth = `A${String.fromCharCode(0x200b)}a`;
    const invisible = await rejection(backend.patchMe({ displayName: zeroWidth }));
    expect(invisible.status).toBe(400);
    expect(invisible.refusalAt('/displayName')?.reason).toBe('invalid_characters');
    const long = await rejection(backend.patchMe({ displayName: 'x'.repeat(65) }));
    expect(long.refusalAt('/displayName')?.reason).toBe('too_long');
    expect((await backend.me()).displayName).toBe('Ada');
  });

  it('is found by the search of people, as the login name is', async () => {
    const { backend, other } = setup();
    const named = await other.patchMe({ displayName: 'Countess Lovelace' });
    const [team] = await backend.teams();
    const found = await backend.searchUsers(team?.id ?? '', 'COUNTESS');
    expect(found.map((user) => user.id)).toEqual([named.id]);
  });

  it('shows in the workspace as "display name (@login)" once changed', async () => {
    const { workspace } = setup();
    await workspace.load();
    const meId = workspace.me?.id ?? '';
    await workspace.changeDisplayName('Ada');
    expect(workspace.me?.displayName).toBe('Ada');
    expect(workspace.userName(meId)).toBe(`Ada (@${workspace.me?.username ?? ''})`);
    await workspace.changeDisplayName('');
    expect(workspace.userName(meId)).toBe(workspace.me?.username);
  });
});

describe('the display name, on the server', () => {
  it('goes as patchMe, and comes back as the user', async () => {
    const sent: { url: string; method: string; body: unknown }[] = [];
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      sent.push({
        url,
        method: init.method ?? 'GET',
        body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined,
      });
      const user = {
        id: 'u1',
        username: 'ada',
        displayName: 'Ada',
        isAdmin: false,
        createAt: 1,
        updateAt: 2,
        deleteAt: 0,
      };
      return Promise.resolve(
        new Response(JSON.stringify(user), { headers: { 'Content-Type': 'application/json' } }),
      );
    });
    const user = await new ServerBackend('/base').patchMe({ displayName: 'Ada' });
    expect(sent).toEqual([
      { url: '/base/api/v1/users/me', method: 'PATCH', body: { displayName: 'Ada' } },
    ]);
    expect(user).toMatchObject({ id: 'u1', username: 'ada', displayName: 'Ada' });
  });

  it('is sent at registration only when one was typed', async () => {
    const sent: unknown[] = [];
    vi.stubGlobal('fetch', (_url: string, init: RequestInit) => {
      sent.push(typeof init.body === 'string' ? JSON.parse(init.body) : undefined);
      return Promise.resolve(new Response(null, { status: 204 }));
    });
    const backend = new ServerBackend('');
    await backend.register({ username: 'a', email: 'a@x', password: 'p', displayName: 'A' });
    await backend.register({ username: 'b', email: 'b@x', password: 'p', displayName: '' });
    expect(sent).toEqual([
      { username: 'a', email: 'a@x', password: 'p', displayName: 'A' },
      { username: 'b', email: 'b@x', password: 'p' },
    ]);
  });
});
