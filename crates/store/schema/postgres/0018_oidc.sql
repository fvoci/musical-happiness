-- Signing in through an organization's OpenID Connect provider.
--
-- An organization has at most one provider. A person who signs in through one is known
-- by the provider's issuer and the subject it gives them, never by their email address:
-- `user_identities` ties that pair to an account, which, like `users`, is nobody's. A
-- sign-in under way is a row of `oidc_login_flows`, taken and removed by the request
-- that comes back from the provider. A session says which provider it was made through.
--
-- The provider and the flows are an organization's rows, behind the wall as the tables
-- of 0014 are: a row gets the organization of the transaction that stores it, and a
-- flow names the provider of its own organization. The line that follows is there
-- because the check of later migrations sees the words of the policies
-- (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE oidc_providers (
    id UUID NOT NULL PRIMARY KEY,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid
        REFERENCES organizations (id) ON DELETE CASCADE,
    -- The issuer as the provider names itself in its discovery document, exactly.
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    -- The client secret, sealed with the server's `secret_key` (AES-256-GCM, bound to
    -- the organization, the issuer and the client), as text; empty for a client
    -- without a secret. Never the secret itself.
    client_secret TEXT NOT NULL,
    -- Whether somebody the provider vouches for who is no member yet becomes one, of
    -- the organization and of its default workspace.
    auto_join BOOLEAN NOT NULL DEFAULT true,
    enabled BOOLEAN NOT NULL DEFAULT true,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    UNIQUE (id, org_id)
);
CREATE UNIQUE INDEX oidc_providers_org_id_key ON oidc_providers (org_id);

CREATE TABLE oidc_login_flows (
    -- The SHA-256 of the `state` sent to the provider, in hexadecimal: the row is found
    -- by what comes back, and what is stored is no `state` to come back with.
    state_hash TEXT NOT NULL PRIMARY KEY,
    -- The SHA-256 of the secret in the cookie of the browser that started the flow.
    browser_hash TEXT NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid
        REFERENCES organizations (id) ON DELETE CASCADE,
    provider_id UUID NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    FOREIGN KEY (provider_id, org_id) REFERENCES oidc_providers (id, org_id) ON DELETE CASCADE
);
CREATE INDEX oidc_login_flows_expires_at_idx ON oidc_login_flows (expires_at);

DO $$
DECLARE
    tbl TEXT;
BEGIN
    FOREACH tbl IN ARRAY ARRAY['oidc_providers', 'oidc_login_flows'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', tbl);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', tbl);
        EXECUTE format(
            'CREATE POLICY org_wall ON %I USING ('
            'org_id = (SELECT NULLIF(current_setting(''fvoci.org'', true), '''')::uuid) '
            'OR (SELECT current_setting(''fvoci.system'', true)) = ''on'')', tbl);
    END LOOP;
END $$;

-- Who a provider vouches for: one account for one subject of one issuer.
CREATE TABLE user_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- The email address the provider gave at the first sign-in, as it gave it: its word,
    -- checked by nobody; empty for none. No key: two providers may give the same.
    email TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    PRIMARY KEY (issuer, subject)
);
CREATE INDEX user_identities_user_id_idx ON user_identities (user_id);

-- An account made through a provider has no email address of its own: the address the
-- provider gave is its word alone, kept with the identity above, and it holds no place
-- among the addresses of the accounts. Those stay unique, the empty one aside.
DROP INDEX users_email_key;
CREATE UNIQUE INDEX users_email_key ON users (lower(email)) WHERE email <> '';

-- The provider a session was made through; NULL for a password. No key: a session
-- outlives the provider's row, and then names a provider that is gone.
ALTER TABLE sessions ADD COLUMN login_provider_id UUID;
