/** Confirming a registration's address over HTTP: its tokens go in bodies, never in addresses. */
import type { Confirmed, EmailVerification, WaitingAccount } from '../state/email-verification.ts';
import type { Schemas } from './convert.ts';
import type { Http } from './http.ts';

export class ServerEmailVerification implements EmailVerification {
  readonly #http: Http;

  constructor(http: Http) {
    this.#http = http;
  }

  async resend(waiting: WaitingAccount, language: string): Promise<void> {
    const json: Schemas['RequestEmailVerification'] = {
      username: waiting.username ?? '',
      email: waiting.email ?? '',
      language,
    };
    await this.#http.request('/api/v1/email-verification/request', {}, { method: 'POST', json });
  }

  async complete(token: string): Promise<Confirmed> {
    const json: Schemas['CompleteEmailVerification'] = { token };
    const confirmed = await this.#http.request<Schemas['EmailVerified']>(
      '/api/v1/email-verification/complete',
      {},
      { method: 'POST', json },
    );
    return {
      workspaceId: confirmed.workspaceId,
      ...(confirmed.boardId ? { boardId: confirmed.boardId } : {}),
      organizationStarted: confirmed.organizationStarted,
    };
  }

  /** Starts the tour of a new organization's first workspace: its welcome board. */
  async welcome(workspaceId: string): Promise<string> {
    const started = await this.#http.request<Schemas['OnboardingStart']>(
      '/api/v1/w/{workspaceId}/onboard',
      { workspaceId },
      { method: 'POST' },
    );
    return started.boardId;
  }
}
