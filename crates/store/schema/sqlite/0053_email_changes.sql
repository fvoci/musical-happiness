-- An account's change of its email address that waits for the new address to be
-- confirmed: one for an account, and a new one replaces it. Until it is confirmed the
-- account's address is the one it had, and nothing holds the new one: whether it is
-- free is asked when it is confirmed. It is the account's, which is global, as a reset
-- credential is, and goes wherever one goes: with a new password, and with the account.
-- It ends at `expires_at`, seven days after it was asked for.
CREATE TABLE email_changes (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    new_email TEXT NOT NULL,
    -- The digest of the one link of its mail that confirms it, and when that ends; a
    -- new mail replaces both. NULL until a mail was written.
    token_hash TEXT,
    token_expires_at BIGINT,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    CHECK ((token_hash IS NULL) = (token_expires_at IS NULL))
);
CREATE UNIQUE INDEX email_changes_token_hash_key ON email_changes (token_hash);
CREATE INDEX email_changes_expires_at_idx ON email_changes (expires_at);
