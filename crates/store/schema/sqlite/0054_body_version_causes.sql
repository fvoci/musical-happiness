-- How each version of a card's body came to be kept, as
-- schema/postgres/0054_body_version_causes.sql says.
ALTER TABLE body_versions ADD COLUMN cause TEXT NOT NULL DEFAULT 'auto'
    CHECK (cause IN ('auto', 'manual', 'restored'));
