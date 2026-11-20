SELECT set_config('fvoci.system', 'on', true);

-- Who made an organization that people work in together, and who is in a space.
--
-- `created_by` is the account that made an organization: by signing up with its name,
-- or later, as an account that was there. An account makes a few and no more; the
-- server counts the ones in use it made, here. NULL for the space of an account,
-- which is its owner's, for an organization that was there before this, whose maker
-- nobody wrote down, and for one whose maker's account is gone.
ALTER TABLE organizations ADD COLUMN created_by UUID REFERENCES users (id) ON DELETE SET NULL;
CREATE INDEX organizations_created_by_idx ON organizations (created_by)
    WHERE created_by IS NOT NULL;

-- The organizations of people by age: the oldest is the instance's own, and the login
-- page asks for it before anybody signs in. The spaces, one for each account, are not
-- read for that.
CREATE INDEX organizations_of_people_idx ON organizations (create_at, id) WHERE kind = 'team';

-- A space has its owner in it and nobody else, and no link lets anybody in: whatever
-- the server does. The organization a row names is the one the transaction reaches.
CREATE FUNCTION spaces_take_their_owner_alone() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM organizations AS o
               WHERE o.id = NEW.org_id AND o.kind = 'personal'
                 AND o.owner_id IS DISTINCT FROM NEW.user_id) THEN
        RAISE EXCEPTION 'the space of an account has its owner in it and nobody else'
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER organization_members_of_a_space BEFORE INSERT OR UPDATE OF org_id, user_id
    ON organization_members
    FOR EACH ROW EXECUTE FUNCTION spaces_take_their_owner_alone();
CREATE TRIGGER workspace_members_of_a_space BEFORE INSERT OR UPDATE OF org_id, user_id
    ON workspace_members
    FOR EACH ROW EXECUTE FUNCTION spaces_take_their_owner_alone();

CREATE FUNCTION spaces_have_no_invitations() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM organizations AS o
               WHERE o.id = NEW.org_id AND o.kind = 'personal') THEN
        RAISE EXCEPTION 'nobody is invited into the space of an account'
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER invitations_of_a_space BEFORE INSERT OR UPDATE OF org_id ON invitations
    FOR EACH ROW EXECUTE FUNCTION spaces_have_no_invitations();
