-- The texts of a comment, from the one it was posted with to the one it has now, once
-- its body was changed: a row per version, numbered from 1. A comment whose body was
-- never changed has none. The server keeps the first and the newest (setting
-- max_comment_versions). The rows stay while the comment is in the trash and go when it
-- is gone for good.
CREATE TABLE comment_versions (
    block_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    -- The body; NULL for none.
    body TEXT,
    -- The stored names of the files the body shows, as a JSON list: they are not
    -- collected while the version is kept.
    files TEXT NOT NULL,
    -- Who wrote this version; NULL if nobody known.
    written_by TEXT,
    written_at BIGINT NOT NULL,
    PRIMARY KEY (block_id, version)
);
