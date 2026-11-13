/**
 * The page of an invitation: what it says of the link, and how whoever reads
 * it gets in (signed in: a press; signed out: an account to make, or the one
 * they have). And the sign-in page of a server that takes nobody without one.
 */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import InvitePage from '$lib/features/account/InvitePage.svelte';
import SignInPage from '$lib/features/account/SignInPage.svelte';
import {
  BackendError,
  type AcceptedInvitation,
  type InvitationPreview,
  type NewAccount,
} from '$lib/state/index.ts';

afterEach(cleanup);

const GONE = new BackendError(404, { detail: 'no', reason: 'invitation_unavailable' });

function mount(options: {
  preview?: () => Promise<InvitationPreview>;
  accept?: (token: string, account?: NewAccount) => Promise<AcceptedInvitation>;
  signedInAs?: string;
  onSignIn?: (login: string, password: string) => Promise<void>;
}) {
  const accept = vi.fn(options.accept ?? (() => Promise.resolve({ workspaceId: 'w1' })));
  const onSignIn = vi.fn(options.onSignIn ?? (() => Promise.resolve()));
  const onJoined = vi.fn();
  const onSessionEnded = vi.fn();
  render(InvitePage, {
    props: {
      token: 'the-token',
      signedInAs: options.signedInAs,
      preview:
        options.preview ?? (() => Promise.resolve({ workspaceName: 'Acme', inviterName: 'Ada' })),
      accept,
      onSignIn,
      onJoined,
      onSessionEnded,
      homeHref: '/',
    },
  });
  return { accept, onSignIn, onJoined, onSessionEnded };
}

async function fill(label: RegExp, value: string): Promise<void> {
  await fireEvent.input(screen.getByLabelText(label), { target: { value } });
}

describe('the page of an invitation', () => {
  it('says who invites and where to, and lets in whoever is signed in with one press', async () => {
    const { accept, onSignIn, onJoined } = mount({ signedInAs: 'Grace (@grace)' });
    expect(await screen.findByTestId('invite-summary')).toHaveTextContent(
      'Ada invited you to Acme',
    );
    expect(screen.getByText('You are signed in as Grace (@grace).')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Join' }));
    await waitFor(() => {
      expect(onJoined).toHaveBeenCalledExactlyOnceWith({ workspaceId: 'w1' });
    });
    expect(accept).toHaveBeenCalledExactlyOnceWith('the-token');
    expect(onSignIn).not.toHaveBeenCalled();
  });

  it('names the board a guest is invited to', async () => {
    mount({
      preview: () =>
        Promise.resolve({ workspaceName: 'Acme', inviterName: 'Ada', boardTitle: 'Roadmap' }),
    });
    expect(await screen.findByTestId('invite-summary')).toHaveTextContent(
      'Ada invited you to the board “Roadmap” of Acme',
    );
  });

  it('makes an account for whoever has none, lets it in, and signs it in', async () => {
    const order: string[] = [];
    const { accept, onSignIn, onJoined } = mount({
      accept: () => {
        order.push('accept');
        return Promise.resolve({ workspaceId: 'w1', boardId: 'b1' });
      },
      onSignIn: () => {
        order.push('signIn');
        return Promise.resolve();
      },
    });
    await screen.findByTestId('invite-register-form');
    await fill(/^user name$/i, ' grace ');
    await fill(/^display name$/i, 'Grace Hopper');
    await fill(/email/i, ' grace@example.test ');
    await fill(/^password$/i, 'a long password');
    await fireEvent.click(screen.getByRole('button', { name: 'Create account and join' }));
    await waitFor(() => {
      expect(onJoined).toHaveBeenCalledExactlyOnceWith({ workspaceId: 'w1', boardId: 'b1' });
    });
    expect(accept).toHaveBeenCalledExactlyOnceWith('the-token', {
      username: 'grace',
      displayName: 'Grace Hopper',
      email: 'grace@example.test',
      password: 'a long password',
    });
    expect(onSignIn).toHaveBeenCalledExactlyOnceWith('grace', 'a long password');
    expect(order).toEqual(['accept', 'signIn']);
  });

  it('says what is wrong with an account beside its fields', async () => {
    const { onJoined } = mount({
      accept: () =>
        Promise.reject(
          new BackendError(409, {
            detail: 'taken',
            reason: 'username_taken',
            errors: [{ pointer: '/account/username', detail: 'taken', reason: 'username_taken' }],
          }),
        ),
    });
    await screen.findByTestId('invite-register-form');
    await fill(/^user name$/i, 'grace');
    await fill(/email/i, 'grace@example.test');
    await fill(/^password$/i, 'a long password');
    await fireEvent.click(screen.getByRole('button', { name: 'Create account and join' }));
    expect(await screen.findByTestId('problem-username')).toHaveTextContent(
      'That user name is taken.',
    );
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(onJoined).not.toHaveBeenCalled();
  });

  it('signs in whoever has an account, and lets that in', async () => {
    const { accept, onSignIn, onJoined } = mount({});
    await screen.findByTestId('invite-register-form');
    await fireEvent.click(screen.getByRole('button', { name: 'Have an account? Sign in' }));
    await fill(/user name or email/i, 'grace');
    await fill(/^password$/i, 'a long password');
    await fireEvent.click(screen.getByRole('button', { name: 'Sign in and join' }));
    await waitFor(() => {
      expect(onJoined).toHaveBeenCalledExactlyOnceWith({ workspaceId: 'w1' });
    });
    expect(onSignIn).toHaveBeenCalledExactlyOnceWith('grace', 'a long password');
    expect(accept).toHaveBeenCalledExactlyOnceWith('the-token');
  });

  it('says of a link that lets nobody in only that, with no form', async () => {
    mount({ preview: () => Promise.reject(GONE) });
    expect(await screen.findByTestId('invite-unavailable')).toHaveTextContent(
      'This invite link cannot be used',
    );
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Go to the first page' })).toHaveAttribute('href', '/');
  });

  it('says so too when the link stops letting anybody in while the page is open', async () => {
    const { onJoined } = mount({ signedInAs: 'grace', accept: () => Promise.reject(GONE) });
    await fireEvent.click(await screen.findByRole('button', { name: 'Join' }));
    expect(await screen.findByTestId('invite-unavailable')).toBeInTheDocument();
    expect(onJoined).not.toHaveBeenCalled();
  });

  it('starts again, signed out, when the session ended before the press', async () => {
    const { onJoined, onSessionEnded } = mount({
      signedInAs: 'grace',
      accept: () => Promise.reject(new BackendError(401, { detail: 'no session' })),
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Join' }));
    await waitFor(() => {
      expect(onSessionEnded).toHaveBeenCalledOnce();
    });
    expect(onJoined).not.toHaveBeenCalled();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('offers to try again when the link could not be read at all', async () => {
    mount({ preview: () => Promise.reject(new Error('offline')) });
    expect(await screen.findByRole('button', { name: 'Try again' })).toBeInTheDocument();
    expect(screen.queryByTestId('invite-unavailable')).not.toBeInTheDocument();
  });
});

describe('the sign-in page and who may make an account', () => {
  function signInPage(registrationOpen: boolean) {
    const onRegister = vi.fn(() => Promise.resolve());
    const onSignIn = vi.fn(() => Promise.resolve());
    render(SignInPage, {
      props: { onSignIn, onRegister, optionsOf: () => Promise.resolve({ registrationOpen }) },
    });
    return { onRegister, onSignIn };
  }

  it('starts an organization with the account where the server takes one', async () => {
    const { onRegister, onSignIn } = signInPage(true);
    await fireEvent.click(await screen.findByRole('button', { name: 'New here? Start a team' }));
    await fill(/^team name$/i, ' Acme ');
    await fill(/^user name$/i, 'ada');
    await fill(/email/i, 'ada@example.test');
    await fill(/^password$/i, 'a long password');
    await fireEvent.click(screen.getByRole('button', { name: 'Start team' }));
    await waitFor(() => {
      expect(onSignIn).toHaveBeenCalledExactlyOnceWith('ada', 'a long password');
    });
    expect(onRegister).toHaveBeenCalledExactlyOnceWith({
      organizationName: 'Acme',
      username: 'ada',
      displayName: '',
      email: 'ada@example.test',
      password: 'a long password',
    });
  });

  it('offers no account where the server has its organization, and says what to ask for', async () => {
    signInPage(false);
    expect(await screen.findByTestId('invitation-needed')).toHaveTextContent(
      'No account yet? Ask an administrator of your team for an invite link.',
    );
    expect(screen.queryByRole('button', { name: /start a team/i })).not.toBeInTheDocument();
  });
});
