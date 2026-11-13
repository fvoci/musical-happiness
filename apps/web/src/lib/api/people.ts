/**
 * Who is in a workspace, and the links that let others in, as the server has
 * them (`People` in `lib/state/people-types.ts`). Apart from the rest of the
 * server backend, whose requests about the members of a board have the same names.
 */
import type {
  GuestBoardRole,
  Invitation,
  People,
  WorkspaceMember,
  WorkspaceRole,
} from '../state/people-types.ts';

import type { Schemas } from './convert.ts';
import type { Http } from './http.ts';
import { ServerTeams } from './teams.ts';

/** A link as the pages have it: the members that are left out when they are empty, left out. */
function invitationOf(link: Schemas['Invitation']): Invitation {
  return {
    id: link.id,
    role: link.role === 'guest' ? 'guest' : 'member',
    ...(link.boardId ? { boardId: link.boardId } : {}),
    createdBy: link.createdBy,
    createAt: link.createAt,
    expiresAt: link.expiresAt,
    uses: link.uses,
    ...(link.token ? { token: link.token } : {}),
  };
}

export class ServerPeople implements People {
  readonly #http: Http;
  readonly teams: ServerTeams;

  constructor(http: Http) {
    this.#http = http;
    this.teams = new ServerTeams(http);
  }

  async members(workspaceId: string): Promise<WorkspaceMember[]> {
    return this.#http.request<Schemas['WorkspaceMember'][]>('/api/v1/w/{workspaceId}/members', {
      workspaceId,
    });
  }

  async setRole(
    workspaceId: string,
    userId: string,
    role: WorkspaceRole,
  ): Promise<WorkspaceMember> {
    const request: Schemas['WorkspaceRoleChange'] = { role };
    return this.#http.request<Schemas['WorkspaceMember']>(
      '/api/v1/w/{workspaceId}/members/{userId}',
      { workspaceId, userId },
      { method: 'PUT', json: request },
    );
  }

  async removeMember(workspaceId: string, userId: string): Promise<void> {
    await this.#http.request(
      '/api/v1/w/{workspaceId}/members/{userId}',
      { workspaceId, userId },
      { method: 'DELETE' },
    );
  }

  async invitations(workspaceId: string): Promise<Invitation[]> {
    const listed = await this.#http.request<Schemas['Invitation'][]>(
      '/api/v1/w/{workspaceId}/invitations',
      { workspaceId },
    );
    return listed.map(invitationOf);
  }

  async inviteMembers(workspaceId: string): Promise<Invitation> {
    return this.#invite(workspaceId, { role: 'member' });
  }

  async inviteGuest(
    workspaceId: string,
    boardId: string,
    role: GuestBoardRole,
  ): Promise<Invitation> {
    return this.#invite(workspaceId, { role: 'guest', boardId, boardRole: role });
  }

  async #invite(workspaceId: string, request: Schemas['NewInvitation']): Promise<Invitation> {
    return invitationOf(
      await this.#http.request<Schemas['Invitation']>(
        '/api/v1/w/{workspaceId}/invitations',
        { workspaceId },
        { method: 'POST', json: request },
      ),
    );
  }

  async revokeInvitation(workspaceId: string, invitationId: string): Promise<void> {
    await this.#http.request(
      '/api/v1/w/{workspaceId}/invitations/{invitationId}',
      { workspaceId, invitationId },
      { method: 'DELETE' },
    );
  }

  async guestInvitations(workspaceId: string): Promise<boolean> {
    const workspace = await this.#http.request<Schemas['Workspace']>('/api/v1/w/{workspaceId}', {
      workspaceId,
    });
    return workspace.guestInvitations;
  }

  async setGuestInvitations(workspaceId: string, enabled: boolean): Promise<boolean> {
    const request: Schemas['GuestInvitationsChange'] = { enabled };
    const workspace = await this.#http.request<Schemas['Workspace']>(
      '/api/v1/w/{workspaceId}/guest-invitations',
      { workspaceId },
      { method: 'PUT', json: request },
    );
    return workspace.guestInvitations;
  }
}
