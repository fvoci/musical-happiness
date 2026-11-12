/**
 * Where the reader was going when a sign-in through a provider took them
 * away (`return-to.ts`), and which of their organizations the settings offer
 * to connect (`sign-in-organizations.ts`).
 */
import { beforeEach, describe, expect, it } from 'vitest';

import { keepReturn, takeReturn } from '../../../src/app/return-to.ts';
import { signInOrganizations } from '../../../src/app/sign-in-organizations.ts';

beforeEach(() => {
  sessionStorage.clear();
});

describe('the address to come back to', () => {
  it('is taken once, for a while', () => {
    keepReturn('/w/one/boards/b', 1_000);
    expect(takeReturn(2_000)).toBe('/w/one/boards/b');
    expect(takeReturn(2_000)).toBeUndefined();

    keepReturn('/w/one', 1_000);
    expect(takeReturn(1_000 + 16 * 60 * 1000)).toBeUndefined();
    // Forgotten as it was read, also when it was too old.
    expect(takeReturn(2_000)).toBeUndefined();
  });

  it('is a path of this site only', () => {
    for (const elsewhere of [
      'https://elsewhere.example/',
      '//elsewhere.example/w/x',
      '/\\elsewhere.example/w/x',
      '\\elsewhere.example',
      'w/one',
    ]) {
      keepReturn(elsewhere, 1_000);
      expect(takeReturn(1_001)).toBeUndefined();
    }
    for (const stored of ['"/w/one"', '{"path":"//elsewhere.example","at":1000}', 'no json']) {
      sessionStorage.setItem('fvoci:return-to', stored);
      expect(takeReturn(1_001)).toBeUndefined();
    }
  });
});

describe('the organizations offered to connect', () => {
  it('are those with a provider, once each, by the workspaces the reader knows them by', async () => {
    const asked: string[] = [];
    const found = await signInOrganizations(
      [
        { id: 'a1', title: 'First of A', orgId: 'a' },
        { id: 'a2', title: 'Second of A', orgId: 'a' },
        { id: 'b1', title: '', orgId: 'b' },
        { id: 'c1', title: 'Of C', orgId: 'c' },
        { id: 'd1', title: 'Of D', orgId: 'd' },
        { id: 'm1', title: 'Of none' },
      ],
      (orgId) => {
        asked.push(orgId);
        if (orgId === 'a') return Promise.resolve({ id: 'a', name: '', oidc: true });
        if (orgId === 'b') return Promise.resolve({ id: 'b', name: 'Bee', oidc: true });
        if (orgId === 'c') return Promise.resolve({ id: 'c', name: 'Sea', oidc: false });
        return Promise.reject(new Error('not read'));
      },
    );
    expect(asked).toEqual(['a', 'b', 'c', 'd']);
    expect(found).toEqual([
      { orgId: 'a', name: 'First of A, Second of A' },
      { orgId: 'b', name: 'Bee' },
    ]);
  });
});
