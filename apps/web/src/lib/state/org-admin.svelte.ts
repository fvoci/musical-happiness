/**
 * The organization screen's state: the organization, its workspaces, its
 * members and its records, read together and read again after each change,
 * which the server answers for. A refusal is kept to be told (`refusal`); the
 * lists stay as they were.
 */
import { isBackendError, isSignedOut, ssoOrgOf } from './backend.ts';
import type { OidcProvider, OidcSettings } from './oidc.ts';
import type {
  AuditRecord,
  Organization,
  Organizations,
  OrgMember,
  OrgRole,
  OrgWorkspace,
} from './organization.ts';

/**
 * `sso`: the organization takes no password and the session was made another
 * way; it opens by signing in through its single sign-on, not by asking again.
 */
export type OrgStatus = 'loading' | 'ready' | 'missing' | 'sso' | 'failed';

/** What the page around the screen is told: the workspace's `report` and cache eviction. */
interface Shell {
  report(action: 'load', error: unknown): void;
  evictWorkspace?(workspaceId: string): void;
  evictOrg?(orgId: string): void;
}

/** Why a change was refused, as the screen tells it. */
export type OrgRefusal =
  'name_taken' | 'default_workspace' | 'last_org_admin' | 'not_found' | 'failed';

export function refusalOf(error: unknown): OrgRefusal {
  if (!isBackendError(error)) return 'failed';
  switch (error.reason) {
    case 'workspace_name_taken':
      return 'name_taken';
    case 'default_workspace':
      return 'default_workspace';
    case 'last_org_admin':
      return 'last_org_admin';
    default:
      return error.status === 404 ? 'not_found' : 'failed';
  }
}

export class OrgAdmin {
  readonly orgId: string;
  readonly #api: Organizations;

  status = $state<OrgStatus>('loading');
  organization = $state.raw<Organization | undefined>(undefined);
  workspaces = $state.raw<OrgWorkspace[]>([]);
  members = $state.raw<OrgMember[]>([]);
  records = $state.raw<AuditRecord[]>([]);
  /** What the last change came to when it was refused; undefined after one that was not. */
  refusal = $state<OrgRefusal | undefined>(undefined);
  /** A change is under way. */
  busy = $state(false);

  readonly #shell: Shell | undefined;

  /**
   * `shell` is told of a read refused because the session is over: one that
   * ended with what was changed here (the provider it was made through,
   * removed or given another issuer), for the page to go to signing in.
   */
  constructor(api: Organizations, orgId: string, shell?: Shell) {
    this.#api = api;
    this.orgId = orgId;
    this.#shell = shell;
  }

  readonly inUse = $derived(this.workspaces.filter((workspace) => workspace.deletedAt === 0));
  readonly inTheTrash = $derived(this.workspaces.filter((workspace) => workspace.deletedAt > 0));

  async load(): Promise<void> {
    try {
      const [organization, workspaces, members, records] = await Promise.all([
        this.#api.organization(this.orgId),
        this.#api.workspaces(this.orgId),
        this.#api.members(this.orgId),
        this.#api.records(this.orgId),
      ]);
      this.organization = organization;
      this.workspaces = workspaces;
      this.members = members;
      this.records = records;
      this.status = 'ready';
    } catch (error) {
      if (ssoOrgOf(error) !== undefined) {
        this.status = 'sso';
        this.#shell?.evictOrg?.(this.orgId);
        return;
      }
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.#shell?.evictOrg?.(this.orgId);
      }
      this.status = isBackendError(error) && error.status === 404 ? 'missing' : 'failed';
      if (this.status === 'failed' && isSignedOut(error)) this.#shell?.report('load', error);
    }
  }

  /** Runs a change, then reads everything again. False when it was refused. */
  async #change(step: () => Promise<unknown>): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    try {
      await step();
      this.refusal = undefined;
      return true;
    } catch (error) {
      this.refusal = refusalOf(error);
      return false;
    } finally {
      await this.load();
      this.busy = false;
    }
  }

  createWorkspace(name: string): Promise<boolean> {
    return this.#change(() => this.#api.createWorkspace(this.orgId, name));
  }

  oidcProvider(): Promise<OidcProvider | undefined> {
    return this.#api.oidcProvider(this.orgId);
  }

  async saveOidcProvider(settings: OidcSettings): Promise<OidcProvider> {
    try {
      const saved = await this.#api.saveOidcProvider(this.orgId, settings);
      this.#shell?.evictOrg?.(this.orgId);
      return saved;
    } finally {
      await this.load();
    }
  }

  async deleteOidcProvider(): Promise<void> {
    try {
      await this.#api.deleteOidcProvider(this.orgId);
      this.#shell?.evictOrg?.(this.orgId);
    } finally {
      await this.load();
    }
  }

  async discardOidcPending(): Promise<void> {
    try {
      await this.#api.discardOidcPending(this.orgId);
    } finally {
      await this.load();
    }
  }

  async endOidcFormer(createAt: number): Promise<void> {
    try {
      await this.#api.endOidcFormer(this.orgId, createAt);
      this.#shell?.evictOrg?.(this.orgId);
    } finally {
      await this.load();
    }
  }

  /** The address of the provider for a sign-in through the pending settings. */
  startOidcTrial(): Promise<string> {
    return this.#api.startOidcTrial(this.orgId);
  }

  renameWorkspace(workspaceId: string, name: string): Promise<boolean> {
    return this.#change(() => this.#api.renameWorkspace(this.orgId, workspaceId, name));
  }

  deleteWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(async () => {
      await this.#api.deleteWorkspace(this.orgId, workspaceId);
      this.#shell?.evictWorkspace?.(workspaceId);
    });
  }

  restoreWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(() => this.#api.restoreWorkspace(this.orgId, workspaceId));
  }

  joinWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(() => this.#api.joinWorkspace(this.orgId, workspaceId));
  }

  leaveWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(async () => {
      await this.#api.leaveWorkspace(this.orgId, workspaceId);
      this.#shell?.evictWorkspace?.(workspaceId);
    });
  }

  setRole(userId: string, role: OrgRole): Promise<boolean> {
    return this.#change(async () => {
      await this.#api.setRole(this.orgId, userId, role);
      this.#shell?.evictOrg?.(this.orgId);
    });
  }

  setPublicLinks(on: boolean): Promise<boolean> {
    return this.#change(() => this.#api.changeOrganization(this.orgId, { publicLinks: on }));
  }
}
