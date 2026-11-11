-- Organizations, their members, the members of workspaces, and the wall between
-- organizations.
--
-- An organization is who a workspace (the table `teams`, until it is renamed) and
-- everything in it belongs to. The server decides who reaches what. Below that, as a
-- safety net, every table that holds what is an organization's carries the
-- organization (`org_id`) and a row-level security policy, `org_wall`, that lets a
-- transaction reach the rows of one organization only: the one named by the setting
-- `fvoci.org`, which the store sets when it begins the transaction. A transaction with
-- the setting `fvoci.system` on reaches the rows of all. With neither, the tables read
-- as empty and take no row.
--
-- A MIGRATION AFTER THIS ONE that changes rows of such a table begins with
--   SELECT set_config('fvoci.system', 'on', true);
-- Without it its UPDATE and DELETE change no row and say nothing
-- (crates/store/src/schema.rs checks this).

CREATE TABLE organizations (
    id UUID NOT NULL PRIMARY KEY,
    -- Empty for none yet: an organization that was not named by anybody.
    name TEXT NOT NULL,
    -- The organization that holds what the server ships with and is nobody's.
    is_system BOOLEAN NOT NULL DEFAULT false,
    -- The workspace somebody who joins the organization gets into. NULL only while the
    -- organization is being made.
    default_workspace_id UUID REFERENCES teams (id),
    -- Whether its members may sign in with a password.
    password_login BOOLEAN NOT NULL DEFAULT true,
    settings JSON NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    -- Zero while the organization is in use.
    delete_at BIGINT NOT NULL DEFAULT 0
);

-- The system organization is one.
CREATE UNIQUE INDEX organizations_system_key ON organizations (is_system) WHERE is_system;

-- Who belongs to an organization, and whether they administer it.
CREATE TABLE organization_members (
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    create_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX organization_members_user_id_idx ON organization_members (user_id);

-- Who is in a workspace. Nobody else knows the workspace is there.
CREATE TABLE workspace_members (
    workspace_id UUID NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member', 'guest')),
    -- Whether an administrator of the organization is here as one, having stepped in,
    -- and the role they had in the workspace before, if any, to go back to.
    via_org_admin BOOLEAN NOT NULL DEFAULT false,
    prior_role TEXT CHECK (prior_role IN ('admin', 'member', 'guest')),
    create_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_id_idx ON workspace_members (user_id);

-- A workspace is an organization's, has a name (empty for none yet), and is marked
-- when it is deleted (delete_at; zero while it is in use).
ALTER TABLE teams ADD COLUMN org_id UUID REFERENCES organizations (id);
ALTER TABLE teams ADD COLUMN name TEXT NOT NULL DEFAULT '';
-- The workspace of the system organization, which holds the built-in templates.
ALTER TABLE teams ADD COLUMN is_system BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE teams ADD COLUMN delete_at BIGINT NOT NULL DEFAULT 0;

-- What was stored before there were organizations is one organization's: the oldest
-- team is its default workspace. The identifier is a version 7 UUID, as the server
-- makes them: the time, the version, and random bits.
INSERT INTO organizations (id, name, default_workspace_id, settings, create_at, update_at)
SELECT (lpad(to_hex(now.millis), 12, '0') || '7'
        || substr(replace(gen_random_uuid()::text, '-', ''), 14))::uuid,
       '', oldest.id, '{}', now.millis, now.millis
FROM (SELECT id FROM teams ORDER BY create_at, id LIMIT 1) AS oldest,
     (SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT AS millis) AS now;
UPDATE teams SET org_id = (SELECT id FROM organizations WHERE NOT is_system);
ALTER TABLE teams ALTER COLUMN org_id SET NOT NULL;
CREATE INDEX teams_org_id_idx ON teams (org_id);

-- Every user is a member of it and of its workspaces, and an administrator of the
-- instance administers both.
INSERT INTO organization_members (org_id, user_id, role, create_at)
SELECT o.id, u.id, CASE WHEN u.is_admin THEN 'admin' ELSE 'member' END, u.create_at
FROM users AS u, organizations AS o WHERE NOT o.is_system;
INSERT INTO workspace_members (workspace_id, user_id, role, create_at)
SELECT t.id, u.id, CASE WHEN u.is_admin THEN 'admin' ELSE 'member' END, u.create_at
FROM users AS u, teams AS t;

-- The system organization and its workspace, with identifiers the server knows
-- (crates/store/src/directory.rs). The built-in templates are theirs, not a team's.
INSERT INTO organizations (id, name, is_system, settings, create_at, update_at)
VALUES ('00000000-0000-7000-8000-00000000a000', '', true, '{}', 0, 0);
INSERT INTO teams (id, org_id, is_system, signup_token, settings, create_at, update_at)
VALUES ('00000000-0000-7000-8000-00000000a001', '00000000-0000-7000-8000-00000000a000',
        true, '', '{}', 0, 0);
UPDATE organizations SET default_workspace_id = '00000000-0000-7000-8000-00000000a001'
WHERE id = '00000000-0000-7000-8000-00000000a000';
UPDATE boards SET team_id = '00000000-0000-7000-8000-00000000a001' WHERE is_builtin;

-- From here on PostgreSQL only: the organization of every row, and the wall.
--
-- The tables below get `org_id` here. The server's statements never name it in them:
-- a row gets the organization of the transaction that stores it, and one stored by a
-- transaction that names no organization is refused, as the column takes no NULL. (In
-- `teams` and `organization_members` the server writes the organization itself, on
-- both databases.)
--
-- The role that owns the tables is the one the server connects as, so the policies
-- are forced on the owner too. No policy calls a function of the schema or reads a
-- table.
DO $$
DECLARE
    by_default CONSTANT TEXT[] := ARRAY[
        'workspace_members', 'boards', 'board_members', 'blocks', 'deleted_boards',
        'deleted_blocks', 'categories', 'category_boards', 'file_info', 'file_refs',
        'sharing', 'subscriptions', 'uploads', 'upload_parts', 'comment_versions'];
    walled CONSTANT TEXT[] := by_default || ARRAY['teams', 'organization_members'];
    system_org CONSTANT UUID := '00000000-0000-7000-8000-00000000a000';
    the_org UUID;
    tbl TEXT;
BEGIN
    SELECT id INTO the_org FROM organizations WHERE NOT is_system;

    FOREACH tbl IN ARRAY by_default LOOP
        EXECUTE format('ALTER TABLE %I ADD COLUMN org_id UUID', tbl);
    END LOOP;

    -- Whose the rows stored so far are: the one organization's, but for the built-in
    -- templates and what hangs on them, which are the system's. That takes out of a
    -- user's reach what the user had on a built-in template under a server that let
    -- one be joined or subscribed to (a membership, a subscription, a file, sharing):
    -- the template is nobody's to change. SQLite, without a wall, leaves it in reach.
    UPDATE boards SET org_id = CASE WHEN is_builtin THEN system_org ELSE the_org END;
    UPDATE workspace_members SET org_id = the_org;
    UPDATE board_members AS m SET org_id = b.org_id FROM boards AS b WHERE b.id = m.board_id;
    UPDATE blocks AS k SET org_id = b.org_id FROM boards AS b WHERE b.id = k.board_id;
    UPDATE file_info AS f SET org_id = b.org_id FROM boards AS b WHERE b.id = f.board_id;
    UPDATE sharing AS s SET org_id = b.org_id FROM boards AS b WHERE b.id = s.board_id;
    UPDATE subscriptions AS s SET org_id = k.org_id FROM blocks AS k WHERE k.id = s.block_id;
    -- Without a key to a board or a block: a row of a board in the trash, or of one
    -- that is gone, is the organization's.
    UPDATE file_refs AS r
    SET org_id = COALESCE((SELECT b.org_id FROM boards AS b WHERE b.id = r.board_id), the_org);
    UPDATE comment_versions AS v
    SET org_id = COALESCE((SELECT k.org_id FROM blocks AS k WHERE k.id = v.block_id), the_org);
    UPDATE deleted_blocks AS d
    SET org_id = COALESCE((SELECT b.org_id FROM boards AS b WHERE b.id = d.board_id), the_org);
    UPDATE deleted_boards SET org_id = the_org;
    UPDATE categories SET org_id = the_org;
    UPDATE category_boards SET org_id = the_org;
    UPDATE uploads SET org_id = the_org;
    UPDATE upload_parts SET org_id = the_org;

    FOREACH tbl IN ARRAY by_default LOOP
        EXECUTE format('ALTER TABLE %I ALTER COLUMN org_id SET NOT NULL', tbl);
        EXECUTE format(
            'ALTER TABLE %I ALTER COLUMN org_id '
            'SET DEFAULT (NULLIF(current_setting(''fvoci.org'', true), ''''))::uuid', tbl);
    END LOOP;

    FOREACH tbl IN ARRAY walled LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', tbl);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', tbl);
        EXECUTE format(
            'CREATE POLICY org_wall ON %I USING ('
            'org_id = (SELECT NULLIF(current_setting(''fvoci.org'', true), '''')::uuid) '
            'OR (SELECT current_setting(''fvoci.system'', true)) = ''on'')', tbl);
    END LOOP;
END $$;

-- A row and what it hangs on are one organization's. The wall looks at a row's own
-- org_id and nothing else: without these keys a transaction of one organization could
-- store, as its own row, a member of another's workspace or a board in it, and whoever
-- reads the row by its parent would take it for the other's. So a parent is unique by
-- (id, org_id) and its child refers to both; the action of each key is that of the key
-- the table already has to the same parent.
--
-- Left without such a key, as they are without any: the trash (deleted_boards,
-- deleted_blocks), file_refs, comment_versions and uploads.board_id, whose rows outlive
-- what they name; and the board of a category_boards row, which may be a built-in
-- template of the system organization in a user's own category.
--
-- The wall is up by now, and holds the owner of the tables too: without the system's
-- reach the check of a new key against the rows that are there would find no row and
-- pass whatever they are.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE teams ADD CONSTRAINT teams_id_org_id_key UNIQUE (id, org_id);
ALTER TABLE boards ADD CONSTRAINT boards_id_org_id_key UNIQUE (id, org_id);
ALTER TABLE blocks ADD CONSTRAINT blocks_id_org_id_key UNIQUE (id, org_id);
ALTER TABLE categories ADD CONSTRAINT categories_id_org_id_key UNIQUE (id, org_id);
ALTER TABLE uploads ADD CONSTRAINT uploads_id_org_id_key UNIQUE (id, org_id);

-- The workspace somebody who joins an organization gets into is one of its own.
ALTER TABLE organizations ADD CONSTRAINT organizations_default_workspace_org_fkey
    FOREIGN KEY (default_workspace_id, id) REFERENCES teams (id, org_id);
ALTER TABLE workspace_members ADD CONSTRAINT workspace_members_workspace_org_fkey
    FOREIGN KEY (workspace_id, org_id) REFERENCES teams (id, org_id) ON DELETE CASCADE;
ALTER TABLE boards ADD CONSTRAINT boards_team_org_fkey
    FOREIGN KEY (team_id, org_id) REFERENCES teams (id, org_id);
ALTER TABLE categories ADD CONSTRAINT categories_team_org_fkey
    FOREIGN KEY (team_id, org_id) REFERENCES teams (id, org_id);
ALTER TABLE board_members ADD CONSTRAINT board_members_board_org_fkey
    FOREIGN KEY (board_id, org_id) REFERENCES boards (id, org_id) ON DELETE CASCADE;
ALTER TABLE blocks ADD CONSTRAINT blocks_board_org_fkey
    FOREIGN KEY (board_id, org_id) REFERENCES boards (id, org_id);
ALTER TABLE file_info ADD CONSTRAINT file_info_board_org_fkey
    FOREIGN KEY (board_id, org_id) REFERENCES boards (id, org_id) ON DELETE CASCADE;
ALTER TABLE sharing ADD CONSTRAINT sharing_board_org_fkey
    FOREIGN KEY (board_id, org_id) REFERENCES boards (id, org_id) ON DELETE CASCADE;
ALTER TABLE category_boards ADD CONSTRAINT category_boards_category_org_fkey
    FOREIGN KEY (category_id, org_id) REFERENCES categories (id, org_id) ON DELETE CASCADE;
ALTER TABLE subscriptions ADD CONSTRAINT subscriptions_block_org_fkey
    FOREIGN KEY (block_id, org_id) REFERENCES blocks (id, org_id) ON DELETE CASCADE;
ALTER TABLE upload_parts ADD CONSTRAINT upload_parts_upload_org_fkey
    FOREIGN KEY (upload_id, org_id) REFERENCES uploads (id, org_id) ON DELETE CASCADE;

-- An organization itself is behind the same wall: a transaction reaches the row of
-- the organization it names (its name, its settings) and of no other. Which
-- organization there is to name is found out with the system's reach
-- (crates/store/src/directory.rs).
ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE organizations FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON organizations USING (
    id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on'
);

-- The built-in templates are read by every organization: their boards and blocks,
-- which are all the system organization holds. Reading only, and only by a transaction
-- that names an organization; the wall above keeps everybody but the system
-- organization from changing them. The templates carry no files: were they to, their
-- file records would need the same policy to be copied with them.
CREATE POLICY org_templates ON boards FOR SELECT USING (
    org_id = '00000000-0000-7000-8000-00000000a000'
    AND (SELECT NULLIF(current_setting('fvoci.org', true), '')) IS NOT NULL
);
CREATE POLICY org_templates ON blocks FOR SELECT USING (
    org_id = '00000000-0000-7000-8000-00000000a000'
    AND (SELECT NULLIF(current_setting('fvoci.org', true), '')) IS NOT NULL
);
