-- Who made an organization that people work in together, and who is in a space: the
-- same as schema/postgres/0051_organizations_made_by.sql, which says what each is.
ALTER TABLE organizations ADD COLUMN created_by TEXT REFERENCES users (id) ON DELETE SET NULL;
CREATE INDEX organizations_created_by_idx ON organizations (created_by)
    WHERE created_by IS NOT NULL;

CREATE INDEX organizations_of_people_idx ON organizations (create_at, id) WHERE kind = 'team';

-- A space has its owner in it and nobody else, and no link lets anybody in.
CREATE TRIGGER organization_members_of_a_space BEFORE INSERT ON organization_members
WHEN EXISTS (SELECT 1 FROM organizations AS o
             WHERE o.id = NEW.org_id AND o.kind = 'personal' AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;
CREATE TRIGGER organization_members_stay_of_a_space BEFORE UPDATE OF org_id, user_id
    ON organization_members
WHEN EXISTS (SELECT 1 FROM organizations AS o
             WHERE o.id = NEW.org_id AND o.kind = 'personal' AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;
CREATE TRIGGER workspace_members_of_a_space BEFORE INSERT ON workspace_members
WHEN EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
             WHERE w.id = NEW.workspace_id AND o.kind = 'personal'
               AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;
CREATE TRIGGER workspace_members_stay_of_a_space BEFORE UPDATE OF workspace_id, user_id
    ON workspace_members
WHEN EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
             WHERE w.id = NEW.workspace_id AND o.kind = 'personal'
               AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;
CREATE TRIGGER invitations_of_a_space BEFORE INSERT ON invitations
WHEN EXISTS (SELECT 1 FROM organizations AS o WHERE o.id = NEW.org_id AND o.kind = 'personal')
BEGIN
    SELECT RAISE(ABORT, 'nobody is invited into the space of an account');
END;
