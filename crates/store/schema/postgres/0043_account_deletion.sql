-- An account its person asked to delete. It is out of use from the asking (`delete_at`,
-- as a deactivated account is) and waits until `delete_after`: till then whoever
-- signs in to it may take the asking back, and nothing else. Zero for an account
-- nobody asked to delete.
ALTER TABLE users ADD COLUMN delete_after BIGINT NOT NULL DEFAULT 0
    CHECK (delete_after = 0 OR delete_at > 0);
-- When the erasing of the account began: from then on it is not taken back. The
-- erasing is steps, each a transaction, and one that was cut off is taken up again by
-- this mark.
ALTER TABLE users ADD COLUMN erase_started_at BIGINT NOT NULL DEFAULT 0
    CHECK (erase_started_at = 0 OR delete_at > 0);
-- When the account was erased: the row is what is left of it, its identifier alone,
-- which what the person wrote keeps pointing at. It has no name, no address and no
-- password, and is a member of nothing.
ALTER TABLE users ADD COLUMN erased_at BIGINT NOT NULL DEFAULT 0
    CHECK (erased_at = 0 OR erase_started_at > 0);
-- The accounts that wait to be erased, for the housekeeping that erases them.
CREATE INDEX users_leaving_idx ON users (delete_after) WHERE delete_at > 0 AND erased_at = 0;

-- An erased account has no name: a name is one account's among those that have one.
DROP INDEX users_username_key;
CREATE UNIQUE INDEX users_username_key ON users (lower(username)) WHERE username <> '';
