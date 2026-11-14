-- Where a session made through a provider came from: the organization and issuer of
-- that provider, kept when the provider row is replaced or removed. Removing the
-- provider, changing its issuer, or unlinking the identity of that issuer ends the
-- sessions made so. NULL for a session made with a password.
SELECT set_config('fvoci.system', 'on', true);
ALTER TABLE sessions ADD COLUMN login_org_id UUID;
ALTER TABLE sessions ADD COLUMN login_issuer TEXT;

UPDATE sessions SET login_org_id = p.org_id, login_issuer = p.issuer
FROM oidc_providers AS p
WHERE p.id = sessions.login_provider_id;

-- A session whose provider is gone has no origin to answer for: it ends.
DELETE FROM sessions WHERE login_provider_id IS NOT NULL AND login_issuer IS NULL;

ALTER TABLE sessions ADD CONSTRAINT sessions_login_origin_check CHECK (
    (login_provider_id IS NULL AND login_org_id IS NULL AND login_issuer IS NULL)
    OR (login_provider_id IS NOT NULL AND login_org_id IS NOT NULL
        AND login_issuer IS NOT NULL));

CREATE INDEX sessions_login_origin_idx ON sessions (login_org_id, login_issuer)
    WHERE login_issuer IS NOT NULL;
