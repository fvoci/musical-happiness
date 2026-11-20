SELECT set_config('fvoci.system', 'on', true);

-- The kind of an organization, and the space of one account.
--
-- An organization is one that people work in together (`team`), as every one was so
-- far, or the space of one account (`personal`): the account is its only member, it
-- has one workspace, and nobody administers it but by owning it. Every account has
-- one, made with the account; the accounts that are there get theirs here.
--
-- `owner_id` is the account a personal organization is of, and NULL for any other.
-- One account owns one organization: that is the key below.
ALTER TABLE organizations ADD COLUMN kind TEXT NOT NULL DEFAULT 'team'
    CHECK (kind IN ('team', 'personal'));
ALTER TABLE organizations ADD COLUMN owner_id UUID REFERENCES users (id);
ALTER TABLE organizations ADD CONSTRAINT organizations_owner_of_personal
    CHECK ((kind = 'personal') = (owner_id IS NOT NULL));
ALTER TABLE organizations ADD CONSTRAINT organizations_system_is_no_ones
    CHECK (NOT is_system OR kind = 'team');
CREATE UNIQUE INDEX organizations_owner_id_key ON organizations (owner_id)
    WHERE owner_id IS NOT NULL;

-- An organization stays of the kind it was made as, and of whom.
CREATE FUNCTION organizations_keep_their_kind() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'an organization does not change its kind or its owner'
        USING ERRCODE = 'restrict_violation';
END $$;
CREATE TRIGGER organizations_keep_their_kind BEFORE UPDATE OF kind, owner_id ON organizations
    FOR EACH ROW
    WHEN (NEW.kind IS DISTINCT FROM OLD.kind OR NEW.owner_id IS DISTINCT FROM OLD.owner_id)
    EXECUTE FUNCTION organizations_keep_their_kind();

-- The space of every account there is: an account in use, and one that waits to be
-- deleted, which may be taken back. Not one whose erasing began: what it had is being
-- removed, and it is given nothing more. The identifiers are version 7 UUIDs, as the
-- server makes them (0014_organizations.sql). The workspace has no name, as the
-- organization has none, and invites no guests.
INSERT INTO organizations (id, name, kind, owner_id, settings, create_at, update_at)
SELECT (lpad(to_hex(now.millis), 12, '0') || '7'
        || substr(replace(gen_random_uuid()::text, '-', ''), 14))::uuid,
       '', 'personal', u.id, '{}', now.millis, now.millis
FROM users AS u,
     (SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT AS millis) AS now
WHERE u.erase_started_at = 0;

INSERT INTO workspaces (id, org_id, name, settings, guest_invitations, create_at, update_at)
SELECT (lpad(to_hex(o.create_at), 12, '0') || '7'
        || substr(replace(gen_random_uuid()::text, '-', ''), 14))::uuid,
       o.id, '', '{}', false, o.create_at, o.create_at
FROM organizations AS o WHERE o.kind = 'personal';

UPDATE organizations AS o SET default_workspace_id = w.id
FROM workspaces AS w WHERE w.org_id = o.id AND o.kind = 'personal';

INSERT INTO organization_members (org_id, user_id, role, create_at)
SELECT o.id, o.owner_id, 'admin', o.create_at
FROM organizations AS o WHERE o.kind = 'personal';

INSERT INTO workspace_members (workspace_id, org_id, user_id, role, create_at)
SELECT o.default_workspace_id, o.id, o.owner_id, 'admin', o.create_at
FROM organizations AS o WHERE o.kind = 'personal';
