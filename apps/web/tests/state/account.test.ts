/**
 * The reader's account beside their settings: the display name, as the
 * memory backend keeps it and the server backend sends it, and as the
 * workspace shows it.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { BackendError, changeName, type User } from '$lib/state/index.ts';

import { serverBackend, settle, setup } from './support.ts';

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
    expect((await backend.patchMe({ customName: '  Ada  ' })).displayName).toBe('Ada');
    expect((await backend.me()).displayName).toBe('Ada');
    expect((await backend.patchMe({ customName: null })).displayName).toBe('');
    expect((await backend.patchMe({ customName: ' ' })).displayName).toBe('');
  });

  it('refuses what the server refuses, at the member, and changes nothing', async () => {
    const { backend } = setup();
    await backend.patchMe({ customName: 'Ada' });
    const zeroWidth = `A${String.fromCharCode(0x200b)}a`;
    const invisible = await rejection(backend.patchMe({ customName: zeroWidth }));
    expect(invisible.status).toBe(400);
    expect(invisible.refusalAt('/customName')?.reason).toBe('invalid_characters');
    const long = await rejection(backend.patchMe({ customName: 'x'.repeat(65) }));
    expect(long.refusalAt('/customName')?.reason).toBe('too_long');
    expect((await backend.me()).displayName).toBe('Ada');
  });

  it('is found by the search of people, as the login name is', async () => {
    const { backend, other } = setup();
    const named = await other.patchMe({ customName: 'Countess Lovelace' });
    const [workspace] = await backend.workspaces();
    const found = await backend.searchUsers(workspace?.id ?? '', 'COUNTESS');
    expect(found.map((user) => user.id)).toEqual([named.id]);
  });

  it('shows in the workspace as "display name (@login)" once changed', async () => {
    const { workspace } = setup();
    await workspace.load();
    const meId = workspace.me?.id ?? '';
    await changeName(workspace, { customName: 'Ada' });
    expect(workspace.me?.displayName).toBe('Ada');
    expect(workspace.userName(meId)).toBe(`Ada (@${workspace.me?.username ?? ''})`);
    await changeName(workspace, { customName: '' });
    expect(workspace.userName(meId)).toBe(workspace.me?.username);
  });
});

describe('a change of a name, told', () => {
  it("shows another person's new name without reading the workspace again", async () => {
    const { workspace, other } = setup();
    await workspace.load();
    const them = await other.me();
    await other.patchMe({ customName: 'Grace', title: 'dr', credentials: ['phd'] });
    await vi.waitFor(() => {
      expect(workspace.user(them.id)?.name?.title).toBe('dr');
    });
    expect(workspace.userName(them.id)).toBe(`Grace (@${them.username})`);
    expect(workspace.user(them.id)?.name?.credentials).toEqual(['phd']);
  });

  it("follows the reader's own change made in another tab, and nothing tells nothing", async () => {
    const { workspace, backend } = setup();
    await workspace.load();
    const told = vi.fn();
    const stop = backend.watchSelf(told);
    await backend.patchMe({});
    expect(told).not.toHaveBeenCalled();
    await backend.patchMe({ customName: 'Ada' });
    await vi.waitFor(() => {
      expect(workspace.me?.displayName).toBe('Ada');
    });
    expect(told).toHaveBeenCalledWith({ kind: 'user', userId: workspace.me?.id });
    stop();
  });

  it('keeps the later of two readings of one person, whichever answers last', async () => {
    const { workspace, backend, other } = setup();
    await workspace.load();
    const them = await other.me();
    await settle();
    const answers: ((users: User[]) => void)[] = [];
    vi.spyOn(backend, 'usersById').mockImplementation(
      () => new Promise<User[]>((resolve) => answers.push(resolve)),
    );
    await other.patchMe({ customName: 'B' });
    await other.patchMe({ customName: 'C' });
    await vi.waitFor(() => {
      expect(answers).toHaveLength(2);
    });
    answers[1]?.([{ ...them, displayName: 'C' }]);
    await settle();
    answers[0]?.([{ ...them, displayName: 'B' }]);
    await settle();
    expect(workspace.user(them.id)?.displayName).toBe('C');
  });

  it('reads everybody held again when changes may have been missed', async () => {
    const { workspace, world, other } = setup();
    await workspace.load();
    const them = await other.me();
    await settle();
    // Changed while the page heard nothing, as when its connection was down.
    const held = world.users.get(them.id);
    if (!held) throw new Error('no such user in the world');
    held.displayName = 'Missed';
    world.tellUsers([workspace.me?.id ?? ''], { kind: 'resync' });
    await vi.waitFor(() => {
      expect(workspace.user(them.id)?.displayName).toBe('Missed');
    });
  });
});

describe('the name, in memory', () => {
  it('is shown by the choice, down the list when it is empty, as the server shows it', async () => {
    const { backend } = setup();
    const kim = await backend.patchMe({
      firstName: ' 영환 ',
      lastName: '김',
      latinFirstName: 'Younghwan',
      latinLastName: 'Kim',
      customName: '',
      title: 'dr',
      credentials: ['phd', 'kr_attorney'],
    });
    expect(kim.displayName).toBe('김영환');
    expect(kim.name).toMatchObject({ firstName: '영환', title: 'dr' });
    expect((await backend.patchMe({ nameShown: 'latin_name' })).displayName).toBe('Younghwan Kim');
    expect((await backend.patchMe({ nameShown: 'username' })).displayName).toBe('');
    const down = await backend.patchMe({
      nameShown: 'latin_name',
      latinFirstName: null,
      latinLastName: '',
    });
    expect(down.displayName).toBe('');
  });

  it('refuses a code off its list, the same credential twice and a fourth', async () => {
    const { backend } = setup();
    const refusals: [Parameters<typeof backend.patchMe>[0], string][] = [
      [{ title: 'sir' as 'dr' }, '/title'],
      [{ credentials: ['phd', 'phd'] }, '/credentials/1'],
      [{ credentials: ['phd', 'md', 'mba', 'ms'] }, '/credentials'],
      [{ nameShown: 'nickname' as 'name' }, '/nameShown'],
      // A Hangul filler is a letter that shows nothing: refused like U+200B.
      [{ firstName: '\u3164' }, '/firstName'],
    ];
    for (const [patch, pointer] of refusals) {
      const refused = await rejection(backend.patchMe(patch));
      expect(refused.status).toBe(400);
      expect(refused.refusalAt(pointer), pointer).toBeDefined();
    }
  });

  it('leaves a member that is undefined, and a choice of null, as they are', async () => {
    const { backend } = setup();
    await backend.patchMe({ firstName: 'Ada', nameShown: 'name' });
    const kept = await backend.patchMe({ firstName: undefined, nameShown: null as never });
    expect([kept.name?.firstName, kept.name?.nameShown]).toEqual(['Ada', 'name']);
  });

  it('finds a person by the parts of their name', async () => {
    const { backend, other } = setup();
    const named = await other.patchMe({
      firstName: '영환',
      lastName: '김',
      latinFirstName: 'Younghwan',
    });
    const [workspace] = await backend.workspaces();
    for (const query of ['김영환', 'younghwan']) {
      const found = await backend.searchUsers(workspace?.id ?? '', query);
      expect(found.map((user) => user.id)).toEqual([named.id]);
    }
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
    const user = await serverBackend('/base').patchMe({ customName: 'Ada' });
    expect(sent).toEqual([
      { url: '/base/api/v1/users/me', method: 'PATCH', body: { customName: 'Ada' } },
    ]);
    expect(user).toMatchObject({ id: 'u1', username: 'ada', displayName: 'Ada' });
  });

  it('is sent with an invitation that is accepted only when one was typed', async () => {
    const sent: unknown[] = [];
    vi.stubGlobal('fetch', (_url: string, init: RequestInit) => {
      sent.push(typeof init.body === 'string' ? JSON.parse(init.body) : undefined);
      return Promise.resolve(Response.json({ workspaceId: 'w1' }));
    });
    const backend = serverBackend('');
    const account = { username: 'a', email: 'a@x', password: 'p' };
    expect(await backend.acceptInvitation('t', { ...account, displayName: '' })).toEqual({
      workspaceId: 'w1',
    });
    await backend.acceptInvitation('t', { ...account, displayName: 'A' });
    // Whoever is signed in accepts with no account.
    await backend.acceptInvitation('t');
    expect(sent).toEqual([
      { token: 't', account },
      { token: 't', account: { ...account, displayName: 'A' } },
      { token: 't' },
    ]);
  });

  it('is sent at registration only when one was typed', async () => {
    const sent: unknown[] = [];
    vi.stubGlobal('fetch', (_url: string, init: RequestInit) => {
      sent.push(typeof init.body === 'string' ? JSON.parse(init.body) : undefined);
      return Promise.resolve(new Response(null, { status: 204 }));
    });
    const backend = serverBackend('');
    const account = { username: 'a', email: 'a@x', password: 'p' };
    await backend.register({ ...account, displayName: 'A', organizationName: 'Acme' });
    await backend.register({ ...account, displayName: '', organizationName: 'Acme' });
    expect(sent).toEqual([
      { ...account, displayName: 'A', organizationName: 'Acme' },
      { ...account, organizationName: 'Acme' },
    ]);
  });
});

describe('the language setting, in memory', () => {
  it('starts at auto, keeps a tag as it was given, and goes back with null', async () => {
    const { backend } = setup();
    expect((await backend.settings()).language).toBe('auto');
    expect((await backend.patchSettings({ language: 'PT-br' })).language).toBe('PT-br');
    expect((await backend.settings()).language).toBe('PT-br');
    expect((await backend.patchSettings({ language: null })).language).toBe('auto');
  });

  it('refuses a tag without the shape, at the member, as the server does', async () => {
    const { backend } = setup();
    const refused = await rejection(backend.patchSettings({ language: 'en-US-US' }));
    expect(refused.status).toBe(400);
    expect(refused.refusalAt('/language')).toBeDefined();
    expect((await backend.settings()).language).toBe('auto');
  });
});
