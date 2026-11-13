/** A group of the nonguest members of one workspace. */
export interface Team {
  id: string;
  workspaceId: string;
  name: string;
  members: { userId: string; username: string; displayName: string }[];
}

/** Teams are visible to nonguest members; only workspace administrators write them. */
export interface Teams {
  list(workspaceId: string): Promise<Team[]>;
  create(workspaceId: string, name: string): Promise<Team>;
  rename(workspaceId: string, teamId: string, name: string): Promise<Team>;
  delete(workspaceId: string, teamId: string): Promise<void>;
  addMember(workspaceId: string, teamId: string, userId: string): Promise<void>;
  removeMember(workspaceId: string, teamId: string, userId: string): Promise<void>;
}
