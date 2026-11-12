/**
 * The people asked for by their identifiers, when the server will not answer
 * for some of them: the others are found, and those are left out.
 */
import { describe, expect, it } from 'vitest';

import { seenUsers } from '$lib/api/seen-users.ts';
import { BackendError } from '$lib/state/problems.ts';

describe('the people the reader sees', () => {
  it('are asked for at once when the server knows them all', async () => {
    const asked: string[][] = [];
    const found = await seenUsers(['a', 'b', 'c'], (ids) => {
      asked.push(ids);
      return Promise.resolve(ids);
    });
    expect(found).toEqual(['a', 'b', 'c']);
    expect(asked).toEqual([['a', 'b', 'c']]);
  });

  it('are found by halves when one of them is nobody the reader sees, who is left out', async () => {
    const hidden = new Set(['b', 'e']);
    const found = await seenUsers(['a', 'b', 'c', 'd', 'e'], (ids) =>
      ids.some((id) => hidden.has(id))
        ? Promise.reject(new BackendError(404, { detail: 'no such user' }))
        : Promise.resolve(ids),
    );
    expect(found.sort()).toEqual(['a', 'c', 'd']);
  });

  it('give any other failure to the caller, and nobody for no identifiers', async () => {
    await expect(
      seenUsers(['a'], () => Promise.reject(new BackendError(500, { detail: 'down' }))),
    ).rejects.toMatchObject({ status: 500 });
    expect(await seenUsers([], () => Promise.reject(new Error('not asked')))).toEqual([]);
  });
});
