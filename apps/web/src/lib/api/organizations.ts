/**
 * The organization's administration on the server (`/org/{orgId}/…`) and the
 * audit log of a workspace (`/w/{workspaceId}/audit`). The members are read
 * whole, page after page; a record list is its newest {@link RECENT_RECORDS}.
 */
import { isBackendError, ssoOrgOf } from '../state/backend.ts';
import { allPages } from '../state/page-reads.ts';
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
import type { OidcProvider, OidcSettings } from '../state/oidc.ts';

/** How many of the newest audit records a screen shows: one page of that many. */
const RECENT_RECORDS = { page: 0, perPage: 200 };

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
        // Not theirs to administer: the organization is not there for them. One that
        // this session does not open (it takes its single sign-on alone) is passed
        // over, for the next that it does.
        const passedOver = isBackendError(error) && error.status === 404;
        if (!passedOver && ssoOrgOf(error) === undefined) throw error;
      }
    }
    return undefined;
  }

  organization(orgId: string): Promise<Organization> {
    return this.#http.request<Schemas['Organization']>('/api/v1/org/{orgId}', { orgId });
  }

  async oidcProvider(orgId: string): Promise<OidcProvider | undefined> {
    try {
      return await this.#http.request<Schemas['OidcProvider']>('/api/v1/org/{orgId}/oidc', {
        orgId,
      });
    } catch (error) {
      if (isBackendError(error) && error.status === 404) return undefined;
      throw error;
    }
  }

  saveOidcProvider(orgId: string, settings: OidcSettings): Promise<OidcProvider> {
    const json: Schemas['OidcProviderSettings'] = settings;
    return this.#http.request<Schemas['OidcProvider']>(
      '/api/v1/org/{orgId}/oidc',
      { orgId },
      { method: 'PUT', json },
    );
  }

  async deleteOidcProvider(orgId: string): Promise<void> {
    await this.#http.request('/api/v1/org/{orgId}/oidc', { orgId }, { method: 'DELETE' });
  }

  async discardOidcPending(orgId: string): Promise<void> {
    await this.#http.request('/api/v1/org/{orgId}/oidc/pending', { orgId }, { method: 'DELETE' });
  }

  async endOidcFormer(orgId: string, createAt: number): Promise<void> {
    const json: Schemas['OidcFormerProviderEnd'] = { createAt };
    await this.#http.request(
      '/api/v1/org/{orgId}/oidc/former',
      { orgId },
      { method: 'DELETE', json },
    );
  }

  async startOidcTrial(orgId: string): Promise<string> {
    const started = await this.#http.request<Schemas['OidcStart']>(
      '/api/v1/org/{orgId}/oidc/pending/sign-in',
      { orgId },
      { method: 'POST' },
    );
    return started.authorizationUrl;
  }

  changeOrganization(orgId: string, patch: OrganizationPatch): Promise<Organization> {
    const json: Schemas['OrganizationPatch'] = patch;
    return this.#http.request<Schemas['Organization']>(
      '/api/v1/org/{orgId}',
      { orgId },
      { method: 'PATCH', json },
    );
  }

  /** Every member, a page after another until one is not full. */
  members(orgId: string): Promise<OrgMember[]> {
    return allPages((paging) =>
      this.#http.request<Schemas['OrgMember'][]>(
        '/api/v1/org/{orgId}/members',
        { orgId },
        { query: { ...paging } },
      ),
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
      { query: RECENT_RECORDS },
    );
  }

  workspaceRecords(workspaceId: string): Promise<AuditRecord[]> {
    return this.#http.request<Schemas['AuditRecord'][]>(
      '/api/v1/w/{workspaceId}/audit',
      { workspaceId },
      { query: RECENT_RECORDS },
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
