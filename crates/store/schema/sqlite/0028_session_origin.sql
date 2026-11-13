-- Where a session made through a provider came from: the organization and issuer of
-- that provider, kept when the provider row is replaced or removed. Removing the
-- provider, changing its issuer, or unlinking the identity of that issuer ends the
-- sessions made so. NULL for a session made with a password.
ALTER TABLE sessions ADD COLUMN login_org_id TEXT;
ALTER TABLE sessions ADD COLUMN login_issuer TEXT;

UPDATE sessions SET
    login_org_id = (SELECT org_id FROM oidc_providers WHERE id = sessions.login_provider_id),
    login_issuer = (SELECT issuer FROM oidc_providers WHERE id = sessions.login_provider_id)
WHERE login_provider_id IS NOT NULL;

-- A session whose provider is gone has no origin to answer for: it ends.
DELETE FROM sessions WHERE login_provider_id IS NOT NULL AND login_issuer IS NULL;

CREATE INDEX sessions_login_origin_idx ON sessions (login_org_id, login_issuer)
    WHERE login_issuer IS NOT NULL;

-- A provider, its organization and its issuer are named together or not at all, as
-- PostgreSQL's check says; SQLite adds no check to a table in place.
CREATE TRIGGER sessions_login_origin_insert BEFORE INSERT ON sessions
WHEN NOT ((NEW.login_provider_id IS NULL AND NEW.login_org_id IS NULL
           AND NEW.login_issuer IS NULL)
       OR (NEW.login_provider_id IS NOT NULL AND NEW.login_org_id IS NOT NULL
           AND NEW.login_issuer IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'a session names its provider, organization and issuer together');
END;
CREATE TRIGGER sessions_login_origin_update BEFORE UPDATE ON sessions
WHEN NOT ((NEW.login_provider_id IS NULL AND NEW.login_org_id IS NULL
           AND NEW.login_issuer IS NULL)
       OR (NEW.login_provider_id IS NOT NULL AND NEW.login_org_id IS NOT NULL
           AND NEW.login_issuer IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'a session names its provider, organization and issuer together');
END;
