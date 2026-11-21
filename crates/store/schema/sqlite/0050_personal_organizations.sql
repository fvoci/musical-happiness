-- The kind of an organization, and the space of one account: the same as
-- schema/postgres/0050_personal_organizations.sql, which says what each is.
--
-- An SQLite database holds one organization that people work in together, and the
-- space of every account beside it: the trigger that refuses a second organization
-- counts those of the kind `team`.
ALTER TABLE organizations ADD COLUMN kind TEXT NOT NULL DEFAULT 'team'
    CHECK (kind IN ('team', 'personal'));
ALTER TABLE organizations ADD COLUMN owner_id TEXT REFERENCES users (id)
    CHECK ((kind = 'personal') = (owner_id IS NOT NULL) AND (NOT is_system OR kind = 'team'));
CREATE UNIQUE INDEX organizations_owner_id_key ON organizations (owner_id)
    WHERE owner_id IS NOT NULL;

DROP TRIGGER organizations_only_one;
CREATE TRIGGER organizations_only_one BEFORE INSERT ON organizations
WHEN NOT NEW.is_system AND NEW.kind = 'team'
     AND EXISTS (SELECT 1 FROM organizations WHERE NOT is_system AND kind = 'team')
BEGIN
    SELECT RAISE(ABORT, 'an SQLite database holds one organization');
END;
-- An organization stays of the kind it was made as, and of whom: a space that became
-- an organization of people would be a second one past the trigger above.
CREATE TRIGGER organizations_keep_their_kind BEFORE UPDATE OF kind, owner_id ON organizations
WHEN NEW.kind IS NOT OLD.kind OR NEW.owner_id IS NOT OLD.owner_id
BEGIN
    SELECT RAISE(ABORT, 'an organization does not change its kind or its owner');
END;

-- The space of every account there is, but one whose erasing began.
INSERT INTO organizations (id, name, kind, owner_id, settings, create_at, update_at)
SELECT lower(printf('%s-%s-7%s-%s%s-%s',
           substr(now.hex, 1, 8), substr(now.hex, 9, 4), substr(hex(randomblob(2)), 2, 3),
           substr('89ab', 1 + (random() & 3), 1), substr(hex(randomblob(2)), 2, 3),
           hex(randomblob(6)))),
       '', 'personal', u.id, '{}', now.millis, now.millis
FROM users AS u,
     (SELECT millis, printf('%012x', millis) AS hex
      FROM (SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER) AS millis)) AS now
WHERE u.erase_started_at = 0;

INSERT INTO workspaces (id, org_id, name, settings, guest_invitations, create_at, update_at)
SELECT lower(printf('%s-%s-7%s-%s%s-%s',
           substr(printf('%012x', o.create_at), 1, 8), substr(printf('%012x', o.create_at), 9, 4),
           substr(hex(randomblob(2)), 2, 3), substr('89ab', 1 + (random() & 3), 1),
           substr(hex(randomblob(2)), 2, 3), hex(randomblob(6)))),
       o.id, '', '{}', false, o.create_at, o.create_at
FROM organizations AS o WHERE o.kind = 'personal';

UPDATE organizations
SET default_workspace_id = (SELECT w.id FROM workspaces AS w WHERE w.org_id = organizations.id)
WHERE kind = 'personal';

INSERT INTO organization_members (org_id, user_id, role, create_at)
SELECT o.id, o.owner_id, 'admin', o.create_at
FROM organizations AS o WHERE o.kind = 'personal';

INSERT INTO workspace_members (workspace_id, user_id, role, create_at)
SELECT o.default_workspace_id, o.owner_id, 'admin', o.create_at
FROM organizations AS o WHERE o.kind = 'personal';
