-- Registrations whose email address is not confirmed yet: the same as
-- schema/postgres/0034_signups.sql, which says what they are, without the wall.
CREATE TABLE signups (
    id TEXT NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT '',
    organization_name TEXT,
    invitation_id TEXT REFERENCES invitations (id) ON DELETE CASCADE,
    invitation_org_id TEXT,
    token_hash TEXT,
    token_expires_at BIGINT,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
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
