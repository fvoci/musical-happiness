/**
 * What the form that changes the password says about each refusal, and at
 * which field.
 */
import { describe, expect, it } from 'vitest';

import { passwordProblemOf } from '$lib/features/shell/password-change.ts';
import { BackendError } from '$lib/state/index.ts';

const refusal = (status: 400 | 409 | 429 | 500, extra: object = {}) =>
  new BackendError(status, { message: 'no', ...extra });

describe('a refused change of the password', () => {
  it('points at the current password when it is wrong', () => {
    expect(
      passwordProblemOf(
        refusal(400, { reason: 'invalid_credentials', field: 'oldPassword' }),
        'long enough',
      ),
    ).toEqual({ text: 'The current password is wrong.', field: 'current' });
  });

  it('tells a new password that is too short from one that is too long, by the server’s measures', () => {
    const tooShort = refusal(400, { field: 'newPassword' });
    expect(passwordProblemOf(tooShort, 'short')).toEqual({
      text: 'The new password needs at least 8 characters.',
      field: 'next',
    });
    // Seven characters, though fourteen UTF-16 units: too short, as the server counts.
    expect(passwordProblemOf(tooShort, '😀😀😀😀😀😀😀').text).toBe(
      'The new password needs at least 8 characters.',
    );
    expect(passwordProblemOf(tooShort, '가'.repeat(342))).toEqual({
      text: 'The new password is too long.',
      field: 'next',
    });
  });

  it('says what to do after a change elsewhere, too many tries, or anything else', () => {
    expect(passwordProblemOf(refusal(409), 'long enough').text).toBe(
      'The password was changed somewhere else meanwhile. Sign in again with the new one.',
    );
    expect(passwordProblemOf(refusal(429), 'long enough').text).toBe(
      'Too many tries. Wait a moment and try again.',
    );
    expect(passwordProblemOf(refusal(500), 'long enough')).toEqual({
      text: 'That did not work. Try again.',
      field: undefined,
    });
    expect(passwordProblemOf(new Error('bug'), 'long enough').field).toBeUndefined();
  });
});
