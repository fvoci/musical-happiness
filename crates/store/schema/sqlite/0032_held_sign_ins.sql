-- Carrying an account over to the provider an organization moved to: the same as
-- schema/postgres/0032_held_sign_ins.sql, which says what it is, without the wall.
CREATE TABLE oidc_held_sign_ins (
    held_hash TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES oidc_providers (id) ON DELETE CASCADE,
    revision BIGINT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    email TEXT NOT NULL,
    email_unverified BOOLEAN NOT NULL,
    name TEXT NOT NULL,
    preferred_username TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX oidc_held_sign_ins_expires_at_idx ON oidc_held_sign_ins (expires_at);

ALTER TABLE oidc_login_flows ADD COLUMN held_hash TEXT;
