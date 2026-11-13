-- Which state of a provider's sign-in a session proves. `oidc_providers.revision` is
-- raised by every change of what signing in through the provider takes (its issuer,
-- client, secret, and whether it is on), and kept by the others; a session made
-- through the provider keeps the revision it was made at, and shows the provider lets
-- people in at that one only. NULL for a session made with a password, and for one
-- made before this migration, which proves no revision.
ALTER TABLE oidc_providers ADD COLUMN revision BIGINT NOT NULL DEFAULT 1;
ALTER TABLE sessions ADD COLUMN login_revision BIGINT;

-- Who a session made through a provider was made for: the subject the provider gave,
-- beside the issuer of 0028. Unlinking an identity ends the sessions made for that
-- identity, issuer and subject, and no other. NULL for a session made with a
-- password, and for one made before this migration, which knows its issuer only:
-- unlinking an identity of that issuer ends it.
ALTER TABLE sessions ADD COLUMN login_subject TEXT;

-- Only a session made through a provider names a subject or a revision, as
-- PostgreSQL's checks say; SQLite adds no check to a table in place.
CREATE TRIGGER sessions_login_subject_insert BEFORE INSERT ON sessions
WHEN (NEW.login_subject IS NOT NULL OR NEW.login_revision IS NOT NULL)
    AND NEW.login_issuer IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only a session made through a provider names a subject or a revision');
END;
CREATE TRIGGER sessions_login_subject_update
BEFORE UPDATE OF login_subject, login_revision, login_issuer ON sessions
WHEN (NEW.login_subject IS NOT NULL OR NEW.login_revision IS NOT NULL)
    AND NEW.login_issuer IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only a session made through a provider names a subject or a revision');
END;
