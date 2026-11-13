-- Invitations: the links that let somebody into a workspace, as a member of it or as
-- a guest of one of its boards. And the end of the signup token of a workspace, which
-- an invitation takes the place of.
--
-- A link carries a token, 256 random bits; what is stored is the token's digest
-- (SHA-256, hexadecimal), by which a link is found and which is no credential. A link
-- ends at `expires_at`, when it was used `max_uses` times (NULL for no such end), or
-- when it is revoked (`revoked_at`; zero while it is not).
--
-- A row is an organization's, behind the wall as the tables of 0014 are. Which
-- organization a token leads into is found out with the system's reach
-- (crates/store/src/directory.rs). The line that follows is there because the check of
-- migrations sees the names of walled tables in this file (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE invitations (
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    workspace_id UUID NOT NULL,
    -- What whoever accepts becomes in the workspace.
    role TEXT NOT NULL CHECK (role IN ('member', 'guest')),
    -- For a guest: the board they are invited to, and their role on it, which is no
    -- more than a guest may hold.
    board_id UUID,
    board_role TEXT CHECK (board_role IN ('viewer', 'commenter', 'editor')),
    token_hash TEXT NOT NULL,
    -- Who made the link: a user's identifier, kept when the account goes.
    created_by UUID NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    max_uses BIGINT CHECK (max_uses > 0),
    uses BIGINT NOT NULL DEFAULT 0,
    revoked_at BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (id, org_id),
    FOREIGN KEY (workspace_id, org_id) REFERENCES workspaces (id, org_id) ON DELETE CASCADE,
    -- A board that goes, to the trash too, takes the links to it along.
    FOREIGN KEY (board_id, org_id) REFERENCES boards (id, org_id) ON DELETE CASCADE,
    CHECK ((role = 'guest') = (board_id IS NOT NULL)),
    CHECK ((role = 'guest') = (board_role IS NOT NULL))
);
CREATE UNIQUE INDEX invitations_token_hash_key ON invitations (token_hash);
CREATE INDEX invitations_workspace_id_idx ON invitations (workspace_id, create_at);

ALTER TABLE invitations ENABLE ROW LEVEL SECURITY;
ALTER TABLE invitations FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON invitations USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Whether the admins of a workspace's boards may invite guests to them.
ALTER TABLE workspaces ADD COLUMN guest_invitations BOOLEAN NOT NULL DEFAULT true;

-- The signup token let in whoever had it, for good and as often as it was passed on.
ALTER TABLE workspaces DROP COLUMN signup_token;
