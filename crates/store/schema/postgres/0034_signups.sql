SELECT set_config('fvoci.system', 'on', true);

-- A registration whose email address is not confirmed yet, on a server that asks for
-- it (`email_verification`). It is no account: nobody's, in no organization, and no
-- user's lookup finds it. It holds its user name and address against every other
-- account and registration, in any letter case, until it is confirmed (it becomes the
-- account `id`, and starts its organization or joins by its invitation) or it ends at
-- `expires_at`, seven days after it was made, and is removed.
CREATE TABLE signups (
    id UUID NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    -- An Argon2id PHC string.
    password_hash TEXT NOT NULL,
    -- The language of the screen it was made from, for its mail.
    language TEXT NOT NULL DEFAULT '',
    -- The organization a registration of its own starts, and that organization's first
    -- workspace; NULL for one made by the link of an invitation.
    organization_name TEXT,
    -- The link a registration was made by, which counts it as a use from then on: a
    -- registration that ends unconfirmed gives the use back. Gone with the link.
    invitation_id UUID,
    invitation_org_id UUID,
    -- The digest of the one link of its mail that confirms it, and when that ends; a
    -- new mail replaces both.
    token_hash TEXT,
    token_expires_at BIGINT,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    FOREIGN KEY (invitation_id, invitation_org_id)
        REFERENCES invitations (id, org_id) ON DELETE CASCADE,
    CHECK ((organization_name IS NULL) <> (invitation_id IS NULL)),
    CHECK ((invitation_id IS NULL) = (invitation_org_id IS NULL)),
    CHECK ((token_hash IS NULL) = (token_expires_at IS NULL))
);
CREATE UNIQUE INDEX signups_username_key ON signups (lower(username));
CREATE UNIQUE INDEX signups_email_key ON signups (lower(email));
CREATE UNIQUE INDEX signups_token_hash_key ON signups (token_hash);
CREATE INDEX signups_expires_at_idx ON signups (expires_at);
CREATE INDEX signups_invitation_idx ON signups (invitation_id, invitation_org_id)
    WHERE invitation_id IS NOT NULL;
