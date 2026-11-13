-- Invitations, and the end of the signup token of a workspace: the same change as
-- schema/postgres/0022_invitations.sql, which says what they are, without the wall.

CREATE TABLE invitations (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- What whoever accepts becomes in the workspace.
    role TEXT NOT NULL CHECK (role IN ('member', 'guest')),
    -- For a guest: the board they are invited to, and their role on it, which is no
    -- more than a guest may hold.
    board_id TEXT REFERENCES boards (id) ON DELETE CASCADE,
    board_role TEXT CHECK (board_role IN ('viewer', 'commenter', 'editor')),
    token_hash TEXT NOT NULL,
    -- Who made the link: a user's identifier, kept when the account goes.
    created_by TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    max_uses BIGINT CHECK (max_uses > 0),
    uses BIGINT NOT NULL DEFAULT 0,
    revoked_at BIGINT NOT NULL DEFAULT 0,
    CHECK ((role = 'guest') = (board_id IS NOT NULL)),
    CHECK ((role = 'guest') = (board_role IS NOT NULL))
);
CREATE UNIQUE INDEX invitations_token_hash_key ON invitations (token_hash);
CREATE INDEX invitations_workspace_id_idx ON invitations (workspace_id, create_at);

-- Whether the admins of a workspace's boards may invite guests to them.
ALTER TABLE workspaces ADD COLUMN guest_invitations BOOLEAN NOT NULL DEFAULT true;

-- The signup token let in whoever had it, for good and as often as it was passed on.
ALTER TABLE workspaces DROP COLUMN signup_token;
