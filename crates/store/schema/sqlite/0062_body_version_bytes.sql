-- The serialized UTF-8 byte length of each kept body version: the same as
-- schema/postgres/0062_body_version_bytes.sql, which says what it is for.
ALTER TABLE body_versions
    ADD COLUMN body_bytes INTEGER NOT NULL DEFAULT 0 CHECK (body_bytes >= 0);

UPDATE body_versions
SET body_bytes = LENGTH(CAST(body AS BLOB))
WHERE body IS NOT NULL;
