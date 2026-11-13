-- Groups of the nonguest members of one workspace and their board roles.
SELECT set_config('fvoci.system', 'on', true);

CREATE UNIQUE INDEX workspace_members_scope_key ON workspace_members (workspace_id, user_id, org_id);
CREATE UNIQUE INDEX boards_workspace_scope_key ON boards (id, workspace_id, org_id);
CREATE UNIQUE INDEX workspaces_scope_key ON workspaces (id, org_id);

CREATE TABLE teams (
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL,
    name TEXT NOT NULL CHECK (length(name) > 0),
    PRIMARY KEY (id, org_id),
    UNIQUE (id, workspace_id, org_id),
    UNIQUE (workspace_id, name, org_id),
    FOREIGN KEY (workspace_id, org_id) REFERENCES workspaces (id, org_id) ON DELETE CASCADE
);
CREATE TABLE team_members (
    team_id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    workspace_id UUID NOT NULL,
    user_id UUID NOT NULL,
    PRIMARY KEY (team_id, user_id, org_id),
    FOREIGN KEY (team_id, workspace_id, org_id) REFERENCES teams (id, workspace_id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, user_id, org_id) REFERENCES workspace_members (workspace_id, user_id, org_id) ON DELETE CASCADE
);
CREATE INDEX team_members_user_idx ON team_members (user_id, workspace_id, org_id);
CREATE TABLE board_teams (
    board_id UUID NOT NULL,
    team_id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    workspace_id UUID NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('viewer', 'commenter', 'editor')),
    PRIMARY KEY (board_id, team_id, org_id),
    FOREIGN KEY (board_id, workspace_id, org_id) REFERENCES boards (id, workspace_id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (team_id, workspace_id, org_id) REFERENCES teams (id, workspace_id, org_id) ON DELETE CASCADE
);
CREATE INDEX board_teams_team_idx ON board_teams (team_id, org_id);

ALTER TABLE teams ENABLE ROW LEVEL SECURITY;
ALTER TABLE teams FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON teams USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

ALTER TABLE team_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE team_members FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON team_members USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

ALTER TABLE board_teams ENABLE ROW LEVEL SECURITY;
ALTER TABLE board_teams FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON board_teams USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
