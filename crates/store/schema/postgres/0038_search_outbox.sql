-- What the search server is still to be told, and which index of it this database
-- has been keeping up to date.
--
-- A row of `search_outbox` says that something changed, never what: who sends it
-- reads what is stored at that time. It is written in the transaction that changes a
-- block or a board, by a server that has a search server, and taken away once the
-- search server holds what is stored, unless `seq` moved on meanwhile.
--
-- A row is an organization's, behind the wall as the tables of 0014 are. The policy
-- below reads the settings and no row; the line that follows is there because the
-- check of later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE search_outbox (
    -- 'block': the document of the block `id` is to be put, or removed if the block
    -- has none now. 'board': every document of the board `id` is, those of blocks
    -- that are gone too.
    scope TEXT NOT NULL CHECK (scope IN ('block', 'board')),
    id UUID NOT NULL,
    -- The organization of the block or board when the row was written: what is gone
    -- from its table by the time the row is read is still removed as that
    -- organization's. Every statement that writes a row names it, from the row of
    -- the block or the board.
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    -- Moved on by every change of what the row is about. Who sent what was stored
    -- takes the row away only if it still has the `seq` they read.
    seq BIGINT NOT NULL,
    -- When the row is next looked at: at once, or later after a failure.
    due_at BIGINT NOT NULL,
    -- How many times in a row it could not be sent.
    attempts BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, scope, id)
);
CREATE INDEX search_outbox_due_at_idx ON search_outbox (due_at);

ALTER TABLE search_outbox ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_outbox FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON search_outbox USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- The index of the search server that `search_outbox` has been kept for since every
-- board was listed in it: its name, the identifier the search server gave it, and the
-- version of the definition it was made from. No row until that has happened, and
-- none again once a server starts without a search server: an index nothing was
-- listed for is made again.
CREATE TABLE search_index_state (
    id SMALLINT NOT NULL PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL,
    uuid TEXT NOT NULL,
    version BIGINT NOT NULL
);
