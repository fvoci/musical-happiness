-- The Y document of a card's body while people edit it together, as the server stores
-- it: a state and the updates taken since (crates/server/src/collab). The body's JSON
-- in `blocks.body` stays what everything else reads; the server writes the document's
-- body there (its projection) a little after the updates.
--
-- A block's document outlives the block while the block is in the trash, as the
-- versions of a comment do, and goes with it when it is gone for good: the purge of the
-- trash removes a document whose block is neither there nor in the trash. So the
-- document has no key to its block, whose row leaves `blocks` for the trash; the
-- updates have one to their document.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policies
-- below read the settings and no row; the line that follows is there because the check
-- of later migrations sees the word of their subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE body_docs (
    block_id UUID NOT NULL,
    -- The document as a version 1 update of all of it.
    state BYTEA NOT NULL,
    -- The last update the state holds: updates up to it are in the state, and those
    -- after it are in body_updates. 0 for none.
    state_seq BIGINT NOT NULL,
    -- The last update logged, of those that are and that were: numbers are not
    -- given twice.
    log_end BIGINT NOT NULL,
    -- The last update whose body `blocks.body` holds: when it and log_end are the
    -- same, the document's body is in the block.
    projected_seq BIGINT NOT NULL,
    -- The SHA-256 of the body's JSON as the block held it when the two were last the
    -- same (the keys of each object in order, no space); of nothing for no body. A body that is
    -- other than that by the time the document is opened was written without it.
    body_digest BYTEA NOT NULL,
    -- Which making of the document it is: 1 for the first, and one more each time it
    -- is made again from its block's body. An editor's update made against another is
    -- out of step.
    epoch BIGINT NOT NULL,
    -- The user whose editor sent the last update logged, which writes the body to the
    -- block as theirs (the log may be folded before); NULL for none.
    last_writer UUID,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    PRIMARY KEY (block_id, org_id)
);

-- The updates a document took after its state, in the order it took them.
CREATE TABLE body_updates (
    block_id UUID NOT NULL,
    seq BIGINT NOT NULL,
    -- A version 1 update.
    bytes BYTEA NOT NULL,
    -- The user whose editor sent it; NULL for the server's own (what a repair
    -- deleted).
    written_by UUID,
    written_at BIGINT NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    PRIMARY KEY (block_id, org_id, seq),
    FOREIGN KEY (block_id, org_id) REFERENCES body_docs (block_id, org_id) ON DELETE CASCADE
);

ALTER TABLE body_docs ENABLE ROW LEVEL SECURITY;
ALTER TABLE body_docs FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON body_docs USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
ALTER TABLE body_updates ENABLE ROW LEVEL SECURITY;
ALTER TABLE body_updates FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON body_updates USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
