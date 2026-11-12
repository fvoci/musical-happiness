-- What was done that is to be accounted for, kept for the administrators of the
-- organization: for now, a workspace's administrator making themselves a member of a
-- board whose content they could not read. Who did what, to what, in which workspace,
-- and when. Rows are written and never changed; reading them is for a later route.
--
-- A row is an organization's, behind the wall as the tables of 0014 are. The line that
-- follows is there because the check of migrations sees the name of a walled table in
-- this file (crates/store/src/schema.rs); nothing here reads or changes a row.
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE audit_log (
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    workspace_id UUID NOT NULL,
    -- Who did it: a user's identifier, kept when the account goes.
    actor_id UUID NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('self_added_to_board')),
    -- What it was done to: for a membership of a board, the board, kept when it goes.
    target_id UUID NOT NULL,
    create_at BIGINT NOT NULL,
    PRIMARY KEY (id, org_id),
    FOREIGN KEY (workspace_id, org_id) REFERENCES workspaces (id, org_id) ON DELETE CASCADE
);
CREATE INDEX audit_log_workspace_id_idx ON audit_log (workspace_id, create_at);

ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_log FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON audit_log USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
