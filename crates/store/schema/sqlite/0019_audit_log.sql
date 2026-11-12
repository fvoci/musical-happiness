-- What was done that is to be accounted for, kept for the administrators of the
-- organization: for now, a workspace's administrator making themselves a member of a
-- board whose content they could not read. Who did what, to what, in which workspace,
-- and when. Rows are written and never changed; reading them is for a later route.
CREATE TABLE audit_log (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- Who did it: a user's identifier, kept when the account goes.
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('self_added_to_board')),
    -- What it was done to: for a membership of a board, the board, kept when it goes.
    target_id TEXT NOT NULL,
    create_at BIGINT NOT NULL
);
CREATE INDEX audit_log_workspace_id_idx ON audit_log (workspace_id, create_at);

-- Who wrote a block, asked for when a user who left a workspace is named where they
-- wrote (crates/store/src/directory.rs).
CREATE INDEX blocks_created_by_idx ON blocks (created_by);
CREATE INDEX blocks_modified_by_idx ON blocks (modified_by);
