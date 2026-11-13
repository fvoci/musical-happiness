import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import InviteEmail from '$lib/features/shell/InviteEmail.svelte';
import { BackendError } from '$lib/state/index.ts';

afterEach(cleanup);

it('hides email delivery when SMTP is unavailable', async () => {
  const available = vi.fn(() => Promise.resolve(false));
  render(InviteEmail, {
    mail: { available, send: vi.fn() },
    workspaceId: 'workspace',
    token: 'secret',
    language: 'en',
  });
  await waitFor(() => {
    expect(available).toHaveBeenCalledOnce();
  });
  expect(screen.queryByTestId('invite-email-form')).toBeNull();
});

it('sends addresses separately with the existing token and acknowledges queuing', async () => {
  const send = vi.fn(() => Promise.resolve());
  render(InviteEmail, {
    mail: { available: () => Promise.resolve(true), send },
    workspaceId: 'workspace',
    token: 'secret',
    language: 'ko',
  });
  await fireEvent.input(await screen.findByLabelText('Email invitation to'), {
    target: { value: ' one@example.test,\ntwo@example.test ' },
  });
  await fireEvent.submit(screen.getByTestId('invite-email-form'));
  expect(send).toHaveBeenCalledExactlyOnceWith(
    'workspace',
    'secret',
    ['one@example.test', 'two@example.test'],
    'ko',
  );
  expect(await screen.findByRole('status')).toHaveTextContent('Invitation mail queued');
});

it('explains invalid addresses without showing server details', async () => {
  const send = vi.fn(() =>
    Promise.reject(
      new BackendError(400, {
        errors: [{ pointer: '/recipients', reason: 'invalid', detail: 'private detail' }],
      }),
    ),
  );
  render(InviteEmail, {
    mail: { available: () => Promise.resolve(true), send },
    workspaceId: 'workspace',
    token: 'secret',
    language: 'en',
  });
  await fireEvent.input(await screen.findByLabelText('Email invitation to'), {
    target: { value: 'bad address' },
  });
  await fireEvent.submit(screen.getByTestId('invite-email-form'));
  expect(await screen.findByRole('alert')).toHaveTextContent('Enter valid email addresses');
  expect(screen.getByRole('alert')).not.toHaveTextContent('private detail');
});
