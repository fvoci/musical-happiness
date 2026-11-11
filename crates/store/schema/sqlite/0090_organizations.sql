-- Organizations.
--
-- An organization is who a workspace (the table `teams`, until it is renamed) and
-- everything in it belongs to. An SQLite database holds one. PostgreSQL holds several
-- and keeps their rows apart with a column and a policy on every table of theirs
-- (schema/postgres/0090_organizations.sql); SQLite has neither.

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
