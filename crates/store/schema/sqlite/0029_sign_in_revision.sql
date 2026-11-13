-- Who a session made through a provider was made for: the subject the provider gave,
-- beside the issuer of 0028. Unlinking an identity ends the sessions made for that
-- identity, issuer and subject, and no other. NULL for a session made with a
-- password, and for one made before this migration, which knows its issuer only:
-- unlinking an identity of that issuer ends it.
ALTER TABLE sessions ADD COLUMN login_subject TEXT;

-- Only a session made through a provider names a subject, as PostgreSQL's check says;
-- SQLite adds no check to a table in place.
CREATE TRIGGER sessions_login_subject_insert BEFORE INSERT ON sessions
WHEN NEW.login_subject IS NOT NULL AND NEW.login_issuer IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only a session made through a provider names a subject');
END;
CREATE TRIGGER sessions_login_subject_update
BEFORE UPDATE OF login_subject, login_issuer ON sessions
WHEN NEW.login_subject IS NOT NULL AND NEW.login_issuer IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only a session made through a provider names a subject');
END;
