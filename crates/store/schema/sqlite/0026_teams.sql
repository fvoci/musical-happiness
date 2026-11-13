-- Groups of the nonguest members of one workspace and their board roles.

CREATE UNIQUE INDEX boards_workspace_scope_key ON boards (id, workspace_id);
CREATE UNIQUE INDEX workspaces_scope_key ON workspaces (id, org_id);

CREATE TABLE teams (
    id TEXT NOT NULL,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL CHECK (length(name) > 0),
    PRIMARY KEY (id, org_id),
    UNIQUE (id, workspace_id, org_id),
    UNIQUE (workspace_id, name, org_id),
    FOREIGN KEY (workspace_id, org_id) REFERENCES workspaces (id, org_id) ON DELETE CASCADE
);
CREATE TABLE team_members (
    team_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    PRIMARY KEY (team_id, user_id, org_id),
    FOREIGN KEY (team_id, workspace_id, org_id) REFERENCES teams (id, workspace_id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, user_id) REFERENCES workspace_members (workspace_id, user_id) ON DELETE CASCADE
);
CREATE INDEX team_members_user_idx ON team_members (user_id, workspace_id, org_id);
CREATE TABLE board_teams (
    board_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('viewer', 'commenter', 'editor')),
    PRIMARY KEY (board_id, team_id, org_id),
    FOREIGN KEY (board_id, workspace_id) REFERENCES boards (id, workspace_id) ON DELETE CASCADE,
    FOREIGN KEY (team_id, workspace_id, org_id) REFERENCES teams (id, workspace_id, org_id) ON DELETE CASCADE
);
CREATE INDEX board_teams_team_idx ON board_teams (team_id, org_id);

-- Saved assignments grant nothing until the board is restored.
ALTER TABLE deleted_boards ADD COLUMN team_roles TEXT NOT NULL DEFAULT '[]';
