-- The same as schema/postgres/0048_welcomed_accounts.sql, which says why.
ALTER TABLE users ADD COLUMN welcomed_at BIGINT;
UPDATE users SET welcomed_at = create_at;
