import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';

import ResetPasswordPage from '$lib/features/account/ResetPasswordPage.svelte';
import { BackendError } from '$lib/state/index.ts';
import { parseRoute } from '../../../src/app/router.svelte.ts';

afterEach(() => {
  cleanup();
  history.replaceState(null, '', '/');
});

it('recognizes the reset page under the deployment path', () => {
  expect(parseRoute('/boards/reset-password', 'boards')).toMatchObject({
    passwordReset: true,
    unknown: false,
  });
  expect(parseRoute('/reset-password/secret', '').unknown).toBe(true);
});

it('sets the new password, forgets the secret and offers sign in', async () => {
  history.replaceState(null, '', '/reset-password#secret');
  const complete = vi.fn(() => Promise.resolve());
  render(ResetPasswordPage, { token: 'secret', homeHref: '/', complete });
  await fireEvent.input(screen.getByLabelText('New password'), {
    target: { value: 'New-password' },
  });
  await fireEvent.submit(screen.getByTestId('reset-password-form'));
  expect(complete).toHaveBeenCalledExactlyOnceWith('secret', 'New-password');
  expect(await screen.findByRole('status')).toHaveTextContent('Your password has been reset');
  expect(location.hash).toBe('');
  expect(screen.getByRole('link', { name: 'Back to sign in' })).toHaveAttribute('href', '/');
  expect(screen.queryByLabelText('New password')).toBeNull();
});

it('explains an unavailable link without showing the server detail', async () => {
  const complete = vi.fn(() =>
    Promise.reject(
      new BackendError(400, {
        errors: [{ pointer: '/token', reason: 'invalid', detail: 'internal value' }],
      }),
    ),
  );
  render(ResetPasswordPage, { token: 'expired', homeHref: '/', complete });
  await fireEvent.input(screen.getByLabelText('New password'), {
    target: { value: 'New-password' },
  });
  await fireEvent.submit(screen.getByTestId('reset-password-form'));
  expect(await screen.findByRole('alert')).toHaveTextContent('invalid or has expired');
  expect(screen.getByRole('alert')).not.toHaveTextContent('internal value');
});
