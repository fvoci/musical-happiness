/**
 * The organization's administration on the server (`/org/{orgId}/…`) and the
 * audit log of a workspace (`/w/{workspaceId}/audit`). Lists are read as one
 * page of the most the server gives (200).
 */
import { isBackendError } from '../state/backend.ts';
import type {
  AuditRecord,
  Organization,
  OrganizationPatch,
  Organizations,
  OrgMember,
  OrgRole,
  OrgWorkspace,
} from '../state/organization.ts';

import type { Schemas } from './convert.ts';
import type { Http } from './http.ts';

/** The most one page of the server holds. */
const PAGE = { page: 0, perPage: 200 };

export class ServerOrganizations implements Organizations {
  readonly #http: Http;

  constructor(http: Http) {
    this.#http = http;
  }

  async administered(): Promise<string | undefined> {
    const workspaces = await this.#http.request<Schemas['UserWorkspace'][]>(
      '/api/v1/users/me/workspaces',
    );
    for (const orgId of new Set(workspaces.map((workspace) => workspace.orgId))) {
      try {
        await this.organization(orgId);
        return orgId;
      } catch (error) {
        // Not theirs to administer: the organization is not there for them.
        if (!isBackendError(error) || error.status !== 404) throw error;
      }
    }
    return undefined;
  }

  organization(orgId: string): Promise<Organization> {
    return this.#http.request<Schemas['Organization']>('/api/v1/org/{orgId}', { orgId });
  }

  changeOrganization(orgId: string, patch: OrganizationPatch): Promise<Organization> {
    const json: Schemas['OrganizationPatch'] = patch;
    return this.#http.request<Schemas['Organization']>(
      '/api/v1/org/{orgId}',
      { orgId },
      { method: 'PATCH', json },
    );
  }

  members(orgId: string): Promise<OrgMember[]> {
    return this.#http.request<Schemas['OrgMember'][]>(
      '/api/v1/org/{orgId}/members',
      { orgId },
      { query: PAGE },
    );
  }

  async setRole(orgId: string, userId: string, role: OrgRole): Promise<void> {
    const json: Schemas['OrgRoleChange'] = { role };
    await this.#http.request(
      '/api/v1/org/{orgId}/members/{userId}/role',
      { orgId, userId },
      { method: 'PUT', json },
    );
  }

  workspaces(orgId: string): Promise<OrgWorkspace[]> {
    return this.#http.request<Schemas['OrgWorkspace'][]>('/api/v1/org/{orgId}/workspaces', {
      orgId,
    });
  }

  createWorkspace(orgId: string, name: string): Promise<OrgWorkspace> {
    const json: Schemas['WorkspaceName'] = { name };
    return this.#http.request<Schemas['OrgWorkspace']>(
      '/api/v1/org/{orgId}/workspaces',
      { orgId },
      { method: 'POST', json },
    );
  }

  renameWorkspace(orgId: string, workspaceId: string, name: string): Promise<OrgWorkspace> {
    const json: Schemas['WorkspaceName'] = { name };
    return this.#http.request<Schemas['OrgWorkspace']>(
      '/api/v1/org/{orgId}/workspaces/{workspaceId}',
      { orgId, workspaceId },
      { method: 'PATCH', json },
    );
  }

  async deleteWorkspace(orgId: string, workspaceId: string): Promise<void> {
    await this.#http.request(
      '/api/v1/org/{orgId}/workspaces/{workspaceId}',
      { orgId, workspaceId },
      { method: 'DELETE' },
    );
  }

  restoreWorkspace(orgId: string, workspaceId: string): Promise<OrgWorkspace> {
    return this.#step(orgId, workspaceId, '/api/v1/org/{orgId}/workspaces/{workspaceId}/restore');
  }

  joinWorkspace(orgId: string, workspaceId: string): Promise<OrgWorkspace> {
    return this.#step(orgId, workspaceId, '/api/v1/org/{orgId}/workspaces/{workspaceId}/join');
  }

  leaveWorkspace(orgId: string, workspaceId: string): Promise<OrgWorkspace> {
    return this.#step(orgId, workspaceId, '/api/v1/org/{orgId}/workspaces/{workspaceId}/leave');
  }

  records(orgId: string): Promise<AuditRecord[]> {
    return this.#http.request<Schemas['AuditRecord'][]>(
      '/api/v1/org/{orgId}/audit',
      { orgId },
      { query: PAGE },
    );
  }

  workspaceRecords(workspaceId: string): Promise<AuditRecord[]> {
    return this.#http.request<Schemas['AuditRecord'][]>(
      '/api/v1/w/{workspaceId}/audit',
      { workspaceId },
      { query: PAGE },
    );
  }

  #step(
    orgId: string,
    workspaceId: string,
    path:
      | '/api/v1/org/{orgId}/workspaces/{workspaceId}/restore'
      | '/api/v1/org/{orgId}/workspaces/{workspaceId}/join'
      | '/api/v1/org/{orgId}/workspaces/{workspaceId}/leave',
  ): Promise<OrgWorkspace> {
    return this.#http.request<Schemas['OrgWorkspace']>(
      path,
      { orgId, workspaceId },
      { method: 'POST' },
    );
  }
}
