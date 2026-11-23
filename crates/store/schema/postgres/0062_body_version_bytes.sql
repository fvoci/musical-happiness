-- The serialized UTF-8 byte length of each kept body version, used to enforce the
-- per-card byte budget (`body_versions_max_bytes`) without loading or scanning the
-- TOASTed `body` column.
--
-- Rows of organizations: the line that follows reaches all of them.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE body_versions
    ADD COLUMN body_bytes BIGINT NOT NULL DEFAULT 0 CHECK (body_bytes >= 0);

UPDATE body_versions
SET body_bytes = OCTET_LENGTH(body::text)
WHERE body IS NOT NULL;
