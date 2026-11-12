-- Signing in through an organization's OpenID Connect provider: the same tables as
-- schema/postgres/0018_oidc.sql, which says what they are, without the wall.

CREATE TABLE oidc_providers (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    auto_join BOOLEAN NOT NULL DEFAULT true,
    enabled BOOLEAN NOT NULL DEFAULT true,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX oidc_providers_org_id_key ON oidc_providers (org_id);

CREATE TABLE oidc_login_flows (
    state_hash TEXT NOT NULL PRIMARY KEY,
    browser_hash TEXT NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES oidc_providers (id) ON DELETE CASCADE,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX oidc_login_flows_expires_at_idx ON oidc_login_flows (expires_at);

CREATE TABLE user_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    create_at BIGINT NOT NULL,
    PRIMARY KEY (issuer, subject)
);
CREATE INDEX user_identities_user_id_idx ON user_identities (user_id);

ALTER TABLE sessions ADD COLUMN login_provider_id TEXT;
