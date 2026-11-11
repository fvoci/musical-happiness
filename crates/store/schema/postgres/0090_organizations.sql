-- Organizations, and the wall between them.
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

-- What was stored before there were organizations is one organization's: the oldest
-- team is its default workspace. The identifier is a version 7 UUID, as the server
-- makes them: the time, the version, and random bits.
INSERT INTO organizations (id, name, default_workspace_id, settings, create_at, update_at)
SELECT (lpad(to_hex(now.millis), 12, '0') || '7'
        || substr(replace(gen_random_uuid()::text, '-', ''), 14))::uuid,
       '', oldest.id, '{}', now.millis, now.millis
FROM (SELECT id FROM teams ORDER BY create_at, id LIMIT 1) AS oldest,
     (SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT AS millis) AS now;

-- The organization of a row, in the tables whose statements do not name it: the
-- organization of the transaction that stores the row. Only PostgreSQL has these
-- columns. The server never reads or writes them; a row stored by a transaction that
-- names no organization is refused, as the column takes no NULL.
ALTER TABLE boards ADD COLUMN org_id UUID;
UPDATE boards SET org_id = (SELECT id FROM organizations WHERE NOT is_system);
ALTER TABLE boards ALTER COLUMN org_id SET NOT NULL;
ALTER TABLE boards ALTER COLUMN org_id
    SET DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid;

-- The wall. The role that owns the tables is the one the server connects as, so the
-- policies are forced on the owner too. No policy calls a function or reads a table.
ALTER TABLE boards ENABLE ROW LEVEL SECURITY;
ALTER TABLE boards FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON boards USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on'
);
