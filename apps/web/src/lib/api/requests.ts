/**
 * The requests of the server backend that take nothing but HTTP: making an
 * account, changing the reader's own, and reading the trash. `ServerBackend`
 * is made of these and the rest (`backend.ts`).
 */
import type { DeletedItem, Paging, Registration, TrashReads, User } from '../state/backend.ts';

import { userOf, type Schemas } from './convert.ts';
import type { Http } from './http.ts';

export class ServerRequests implements TrashReads {
  readonly #http: Http;

  constructor(http: Http) {
    this.#http = http;
  }

  async register(registration: Registration): Promise<void> {
    const request: Schemas['RegisterRequest'] = {
      username: registration.username,
      email: registration.email,
      password: registration.password,
      ...(registration.displayName ? { displayName: registration.displayName } : {}),
      ...(registration.token ? { token: registration.token } : {}),
    };
    await this.#http.request('/api/v1/register', {}, { method: 'POST', json: request });
  }

  async patchMe(patch: { displayName: string | null }): Promise<User> {
    const change: Schemas['AccountChange'] = patch;
    return userOf(
      await this.#http.request<Schemas['User']>(
        '/api/v1/users/me',
        {},
        { method: 'PATCH', json: change },
      ),
    );
  }

  async deletedCards(boardId: string, paging?: Paging): Promise<DeletedItem[]> {
    return this.#http.request<Schemas['DeletedItem'][]>(
      '/api/v1/boards/{boardId}/trash/cards',
      { boardId },
      { query: { ...paging } },
    );
  }

  async deletedBoards(teamId: string, paging?: Paging): Promise<DeletedItem[]> {
    return this.#http.request<Schemas['DeletedItem'][]>(
      '/api/v1/teams/{teamId}/trash/boards',
      { teamId },
      { query: { ...paging } },
    );
  }
}
