-- Organizations, their members, and the members of workspaces.
--
-- An organization is who a workspace (the table `teams`, until it is renamed) and
-- everything in it belongs to. An SQLite database holds one organization, besides the
-- system's. PostgreSQL holds several and keeps their rows apart with a column and a
-- policy on every table of theirs (schema/postgres/0090_organizations.sql); SQLite
-- has neither, so the two files part ways after the tables they share.

CREATE TABLE organizations (
    id TEXT NOT NULL PRIMARY KEY,
    -- Empty for none yet: an organization that was not named by anybody.
    name TEXT NOT NULL,
    -- The organization that holds what the server ships with and is nobody's.
    is_system BOOLEAN NOT NULL DEFAULT false,
    -- The workspace somebody who joins the organization gets into. NULL only while the
    -- organization is being made.
    default_workspace_id TEXT REFERENCES teams (id),
    -- Whether its members may sign in with a password.
    password_login BOOLEAN NOT NULL DEFAULT true,
    settings TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    -- Zero while the organization is in use.
    delete_at BIGINT NOT NULL DEFAULT 0
);

-- The system organization is one.
CREATE UNIQUE INDEX organizations_system_key ON organizations (is_system) WHERE is_system;

-- Who belongs to an organization, and whether they administer it.
CREATE TABLE organization_members (
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    create_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX organization_members_user_id_idx ON organization_members (user_id);

-- Who is in a workspace. Nobody else knows the workspace is there.
CREATE TABLE workspace_members (
    workspace_id TEXT NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member', 'guest')),
    -- Whether an administrator of the organization is here as one, having stepped in,
    -- and the role they had in the workspace before, if any, to go back to.
    via_org_admin BOOLEAN NOT NULL DEFAULT false,
    prior_role TEXT CHECK (prior_role IN ('admin', 'member', 'guest')),
    create_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_id_idx ON workspace_members (user_id);

-- What was stored before there were organizations is one organization's: the oldest
-- team is its default workspace. The identifier is a version 7 UUID, as the server
-- makes them: the time, the version, and random bits.
INSERT INTO organizations (id, name, default_workspace_id, settings, create_at, update_at)
SELECT lower(printf('%s-%s-7%s-%s%s-%s',
           substr(now.hex, 1, 8), substr(now.hex, 9, 4), substr(hex(randomblob(2)), 2, 3),
           substr('89ab', 1 + (random() & 3), 1), substr(hex(randomblob(2)), 2, 3),
           hex(randomblob(6)))),
       '', oldest.id, '{}', now.millis, now.millis
FROM (SELECT id FROM teams ORDER BY create_at, id LIMIT 1) AS oldest,
     (SELECT millis, printf('%012x', millis) AS hex
      FROM (SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER) AS millis)) AS now;

-- A workspace is an organization's, has a name (empty for none yet), and is marked
-- when it is deleted (delete_at; zero while it is in use). SQLite adds no column that
-- must have a value and refers to another table, so the table is made again. The rows
-- that refer to a team (boards, categories, the organization above) are checked when
-- the migration ends, by when their teams are back. Only the checks wait, not what a
-- key does when its team goes: workspace_members, whose rows go with their team, is
-- filled after this, and must be.
PRAGMA defer_foreign_keys = ON;
CREATE TEMPORARY TABLE teams_before AS SELECT * FROM teams;
DROP TABLE teams;
CREATE TABLE teams (
    id TEXT NOT NULL PRIMARY KEY,
    -- A secret; whoever holds it may register.
    signup_token TEXT NOT NULL,
    settings TEXT NOT NULL,
    modified_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    org_id TEXT NOT NULL REFERENCES organizations (id),
    name TEXT NOT NULL DEFAULT '',
    -- The workspace of the system organization, which holds the built-in templates.
    is_system BOOLEAN NOT NULL DEFAULT false,
    delete_at BIGINT NOT NULL DEFAULT 0
);
INSERT INTO teams (id, signup_token, settings, modified_by, create_at, update_at, org_id)
SELECT id, signup_token, settings, modified_by, create_at, update_at,
       (SELECT id FROM organizations WHERE NOT is_system)
FROM teams_before;
DROP TABLE teams_before;
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

-- One organization, besides the system's: a second is refused, and so is making the
-- system's one of the others.
CREATE TRIGGER organizations_only_one BEFORE INSERT ON organizations
WHEN NOT NEW.is_system AND EXISTS (SELECT 1 FROM organizations WHERE NOT is_system)
BEGIN
    SELECT RAISE(ABORT, 'an SQLite database holds one organization');
END;
CREATE TRIGGER organizations_stay_what_they_are BEFORE UPDATE OF is_system ON organizations
WHEN NEW.is_system <> OLD.is_system
BEGIN
    SELECT RAISE(ABORT, 'an organization does not become or stop being the system''s');
END;
