-- Carrying an account over to the provider an organization moved to.
--
-- `oidc_held_sign_ins`: a sign-in through the organization's provider that ended in an
-- identity tied to no account while a former provider is kept
-- (`oidc_former_providers`). It makes no account at once: what the provider said is
-- held for the browser that signed in, for a short while, until the person says
-- whether they had an account (they then sign in through the former provider, and
-- the identity is tied to the account that one vouches for) or start anew (the
-- account is made as at any first sign-in). Taken once, by either. `held_hash` is the
-- SHA-256 of the secret in that browser's cookie, in hexadecimal: what is stored is no
-- secret to come back with. `revision` is the provider's when the identity was
-- vouched for: the session it ends in proves that one, or none. It goes with its
-- provider's row, as the sign-ins under way do.
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE oidc_held_sign_ins (
    held_hash TEXT NOT NULL PRIMARY KEY,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid
        REFERENCES organizations (id) ON DELETE CASCADE,
    provider_id UUID NOT NULL,
    revision BIGINT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    -- What the provider said of the person, as it said it: its word, checked when the
    -- account is made. Empty for what it did not say; `email_unverified` is whether it
    -- said that it has not checked the address.
    email TEXT NOT NULL,
    email_unverified BOOLEAN NOT NULL,
    name TEXT NOT NULL,
    preferred_username TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    FOREIGN KEY (provider_id, org_id) REFERENCES oidc_providers (id, org_id) ON DELETE CASCADE
);
CREATE INDEX oidc_held_sign_ins_expires_at_idx ON oidc_held_sign_ins (expires_at);
ALTER TABLE oidc_held_sign_ins ENABLE ROW LEVEL SECURITY;
ALTER TABLE oidc_held_sign_ins FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON oidc_held_sign_ins USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- A sign-in under way that goes through the former provider, to carry over the held
-- sign-in of this digest; NULL for a sign-in through the provider. It names the
-- provider's row as every sign-in does, and ends with it.
ALTER TABLE oidc_login_flows ADD COLUMN held_hash TEXT;
