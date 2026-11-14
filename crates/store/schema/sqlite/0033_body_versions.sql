-- The versions kept of a card's body: the body as the card held it when the last of
-- its writers left it, every `collab_version_minutes` while it went on changing, and
-- before and after it was written whole. A row per version, numbered from 1 with no
-- number given twice; the server keeps the newest (setting `collab_body_versions`). A
-- version is the body's JSON, as the card's readers are given it, and no document: it
-- reads the same whichever making of the card's document there is.
--
-- The rows stay while the card is in the trash and go when it is gone for good, as the
-- versions of a comment and the card's document do: so they have no key to the block.
CREATE TABLE body_versions (
    block_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    -- The body; NULL for none.
    body TEXT,
    -- The SHA-256 of the body's JSON, as `body_docs.body_digest` is taken: a body that
    -- has the digest of the card's newest version is kept already.
    body_digest BLOB NOT NULL,
    -- The stored names of the files the body shows, as a JSON list: they are not
    -- collected while the version is kept.
    files TEXT NOT NULL,
    kept_at BIGINT NOT NULL,
    PRIMARY KEY (block_id, version)
);

-- Who wrote a card's body up to a version, since the version before: the users whose
-- editors sent updates and those who wrote it whole, never the server for what it
-- repaired. The rows of version 0 are those who wrote since the newest version: the
-- next one takes them.
CREATE TABLE body_version_editors (
    block_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    user_id TEXT NOT NULL,
    PRIMARY KEY (block_id, version, user_id)
);
