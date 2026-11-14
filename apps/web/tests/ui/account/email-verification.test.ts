/**
 * Where the server confirms addresses: the page that says to check the mail
 * after a registration, the refusal of a sign-in to a registration that waits,
 * and the page the mailed link opens.
 */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import InvitePage from '$lib/features/account/InvitePage.svelte';
import SignInPage from '$lib/features/account/SignInPage.svelte';
import VerifyEmailPage from '$lib/features/account/VerifyEmailPage.svelte';
import { BackendError } from '$lib/state/index.ts';
import type { Confirmed } from '$lib/state/email-verification.ts';
import { parseRoute } from '../../../src/app/router.svelte.ts';

afterEach(cleanup);

async function fill(label: RegExp, value: string): Promise<void> {
  await fireEvent.input(screen.getByLabelText(label), { target: { value } });
}

function signInPage(onRegister: () => Promise<'made' | 'waiting'>) {
  const onSignIn = vi.fn(() => Promise.resolve());
  const onResend = vi.fn(() => Promise.resolve());
  render(SignInPage, {
    props: {
      onSignIn,
      onRegister: vi.fn(onRegister),
      onResend,
      optionsOf: () => Promise.resolve({ registrationOpen: true, emailVerification: true }),
    },
  });
  return { onSignIn, onResend };
}

describe('a registration that waits for its address', () => {
  it('says where the link went, sends it again and leads back to the form', async () => {
    const { onSignIn, onResend } = signInPage(() => Promise.resolve('waiting'));
    await fireEvent.click(
      await screen.findByRole('button', { name: 'New here? Start an organization' }),
    );
    await fill(/^organization name$/i, 'Acme');
    await fill(/^user name$/i, 'grace');
    await fill(/email/i, ' grace@example.test ');
    await fill(/^password$/i, 'a long password');
    await fireEvent.submit(screen.getByTestId('register-form'));

    const heading = await screen.findByRole('heading', { name: 'Check your email' });
    expect(heading).toHaveFocus();
    expect(screen.getByTestId('check-mail')).toHaveTextContent('grace@example.test');
    expect(onSignIn).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole('button', { name: 'Send the link again' }));
    await waitFor(() => {
      expect(screen.getByTestId('resend-status')).toHaveTextContent('a new link is on its way');
    });
    expect(onResend).toHaveBeenCalledExactlyOnceWith({ email: 'grace@example.test' });

    await fireEvent.click(screen.getByRole('button', { name: 'Sign up again' }));
    expect(await screen.findByTestId('register-form')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText(/^organization name$/i)).toHaveFocus();
    });
    expect(screen.getByTestId('verify-hint')).toHaveTextContent('We will mail a link');
  });

  it('refuses a sign-in to it with a way to the link, and keeps the wrong password vague', async () => {
    const onResend = vi.fn(() => Promise.resolve());
    const onSignIn = vi.fn(() =>
      Promise.reject(new BackendError(403, { reason: 'email_unverified', detail: 'not yet' })),
    );
    render(SignInPage, {
      props: {
        onSignIn,
        onRegister: vi.fn(() => Promise.resolve('made' as const)),
        onResend,
        optionsOf: () => Promise.resolve({ registrationOpen: true }),
      },
    });
    await fill(/user name or email/i, 'grace');
    await fill(/^password$/i, 'a long password');
    await fireEvent.submit(screen.getByTestId('sign-in-form'));
    expect(await screen.findByRole('alert')).toHaveTextContent('Confirm your email address first');
    expect(screen.getByRole('alert')).not.toHaveTextContent('not yet');
    await fireEvent.click(screen.getByRole('button', { name: 'Send the link again' }));
    await waitFor(() => {
      expect(onResend).toHaveBeenCalledExactlyOnceWith({ username: 'grace', email: 'grace' });
    });
  });

  it('waits for the address of a new account made by an invitation', async () => {
    const onResend = vi.fn(() => Promise.resolve());
    const onJoined = vi.fn();
    render(InvitePage, {
      props: {
        token: 'the-token',
        signedInAs: undefined,
        preview: () => Promise.resolve({ workspaceName: 'Acme', inviterName: 'Ada' }),
        accept: vi.fn(() => Promise.resolve('waiting' as const)),
        onResend,
        onSignIn: vi.fn(() => Promise.resolve()),
        onJoined,
        onSessionEnded: vi.fn(),
        onSso: vi.fn(() => Promise.resolve()),
        homeHref: '/',
      },
    });
    await screen.findByTestId('invite-summary');
    await fill(/^user name$/i, 'grace');
    await fill(/email/i, 'grace@example.test');
    await fill(/^password$/i, 'a long password');
    await fireEvent.click(screen.getByRole('button', { name: 'Create account and join' }));
    expect(await screen.findByTestId('check-mail')).toHaveTextContent('grace@example.test');
    expect(onJoined).not.toHaveBeenCalled();
  });
});

describe('the page of the confirmation link', () => {
  it('is a route of its own under the deployment path', () => {
    expect(parseRoute('/boards/verify-email', 'boards')).toMatchObject({
      emailVerify: true,
      unknown: false,
    });
    expect(parseRoute('/verify-email/secret', '').unknown).toBe(true);
  });

  it('says the confirmation came through, sent once for the token', async () => {
    const complete = vi.fn((_token: string): Promise<Confirmed> =>
      Promise.resolve({ workspaceId: 'w1', organizationStarted: true }),
    );
    render(VerifyEmailPage, { confirming: complete('secret'), homeHref: '/', resend: vi.fn() });
    await waitFor(() => {
      expect(screen.getByTestId('verify-status')).toHaveTextContent('Your address is confirmed');
    });
    expect(complete).toHaveBeenCalledExactlyOnceWith('secret');
  });

  it('says a link that confirms nothing is so, and mails a new one for the address', async () => {
    const complete = vi.fn(() =>
      Promise.reject(
        new BackendError(400, {
          errors: [{ pointer: '/token', reason: 'invalid', detail: 'internal value' }],
        }),
      ),
    );
    const resend = vi.fn(() => Promise.resolve());
    render(VerifyEmailPage, { confirming: complete(), homeHref: '/', resend });
    expect(await screen.findByTestId('verify-problem')).toHaveTextContent(
      'invalid, has expired or was replaced',
    );
    expect(screen.getByTestId('verify-problem')).not.toHaveTextContent('internal value');
    await fill(/email/i, ' grace@example.test ');
    await fireEvent.submit(screen.getByTestId('verify-request-form'));
    await waitFor(() => {
      expect(resend).toHaveBeenCalledExactlyOnceWith('grace@example.test');
    });
    expect(await screen.findByText(/a new link is on its way/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to sign in' })).toHaveAttribute('href', '/');
  });

  it('tells a request that got no answer from a link that confirms nothing', async () => {
    const failed = Promise.reject(new BackendError(0, { detail: 'no answer' }));
    render(VerifyEmailPage, { confirming: failed, homeHref: '/', resend: vi.fn() });
    expect(await screen.findByTestId('verify-problem')).not.toHaveTextContent('invalid');
    expect(screen.queryByTestId('verify-request-form')).toBeNull();
  });

  it('says a link without a token confirms nothing', async () => {
    render(VerifyEmailPage, { confirming: undefined, homeHref: '/', resend: vi.fn() });
    expect(await screen.findByTestId('verify-problem')).toHaveTextContent('invalid');
    expect(screen.getByTestId('verify-request-form')).toBeInTheDocument();
  });

  it('says an invitation that lets nobody in any more is so, without a form', async () => {
    const complete = vi.fn(() =>
      Promise.reject(new BackendError(404, { reason: 'invitation_unavailable', detail: 'x' })),
    );
    render(VerifyEmailPage, { confirming: complete(), homeHref: '/', resend: vi.fn() });
    expect(await screen.findByTestId('verify-problem')).toHaveTextContent(
      'This invite link cannot be used',
    );
    expect(screen.queryByTestId('verify-request-form')).toBeNull();
  });
});
