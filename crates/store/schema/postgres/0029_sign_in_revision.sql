-- Who a session made through a provider was made for: the subject the provider gave,
-- beside the issuer of 0028. Unlinking an identity ends the sessions made for that
-- identity, issuer and subject, and no other. NULL for a session made with a
-- password, and for one made before this migration, which knows its issuer only:
-- unlinking an identity of that issuer ends it.
ALTER TABLE sessions ADD COLUMN login_subject TEXT;
ALTER TABLE sessions ADD CONSTRAINT sessions_login_subject_check
    CHECK (login_subject IS NULL OR login_issuer IS NOT NULL);
