-- An account its person asked to delete, the mark of its erasing and what is left of
-- it: the same as schema/postgres/0043_account_deletion.sql, which says what each is.
ALTER TABLE users ADD COLUMN delete_after BIGINT NOT NULL DEFAULT 0
    CHECK (delete_after = 0 OR delete_at > 0);
ALTER TABLE users ADD COLUMN erase_started_at BIGINT NOT NULL DEFAULT 0
    CHECK (erase_started_at = 0 OR delete_at > 0);
ALTER TABLE users ADD COLUMN erased_at BIGINT NOT NULL DEFAULT 0
    CHECK (erased_at = 0 OR erase_started_at > 0);
CREATE INDEX users_leaving_idx ON users (delete_after) WHERE delete_at > 0 AND erased_at = 0;

DROP INDEX users_username_key;
CREATE UNIQUE INDEX users_username_key ON users (lower(username)) WHERE username <> '';
