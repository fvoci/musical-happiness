/**
 * The organization screen's state: the organization, its workspaces, its
 * members and its records, read together and read again after each change,
 * which the server answers for. A refusal is kept to be told (`refusal`); the
 * lists stay as they were.
 */
import { isBackendError } from './backend.ts';
import type {
  AuditRecord,
  Organization,
  Organizations,
  OrgMember,
  OrgRole,
  OrgWorkspace,
} from './organization.ts';

export type OrgStatus = 'loading' | 'ready' | 'missing' | 'failed';

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

  constructor(api: Organizations, orgId: string) {
    this.#api = api;
    this.orgId = orgId;
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
      this.status = isBackendError(error) && error.status === 404 ? 'missing' : 'failed';
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

  renameWorkspace(workspaceId: string, name: string): Promise<boolean> {
    return this.#change(() => this.#api.renameWorkspace(this.orgId, workspaceId, name));
  }

  deleteWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(() => this.#api.deleteWorkspace(this.orgId, workspaceId));
  }

  restoreWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(() => this.#api.restoreWorkspace(this.orgId, workspaceId));
  }

  joinWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(() => this.#api.joinWorkspace(this.orgId, workspaceId));
  }

  leaveWorkspace(workspaceId: string): Promise<boolean> {
    return this.#change(() => this.#api.leaveWorkspace(this.orgId, workspaceId));
  }

  setRole(userId: string, role: OrgRole): Promise<boolean> {
    return this.#change(() => this.#api.setRole(this.orgId, userId, role));
  }

  setPublicLinks(on: boolean): Promise<boolean> {
    return this.#change(() => this.#api.changeOrganization(this.orgId, { publicLinks: on }));
  }
}
