-- Reset credentials belong to accounts, which are global, as sessions are.
CREATE TABLE password_resets (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
CREATE INDEX password_resets_expires_at_idx ON password_resets (expires_at);
CREATE INDEX password_resets_user_id_idx ON password_resets (user_id);
