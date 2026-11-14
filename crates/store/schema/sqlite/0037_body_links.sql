-- Which block's body names which block: a row per reference node of the body a block
-- holds now (`reference`, whose `id` is the block named), put down again whenever that
-- body is written. `target_id` has no key to a block: a body may name a block that is
-- not there, one of another workspace, one its writer cannot see, and the row says no
-- more than the body does. Who may read the source and the target is asked when the
-- rows are read, of each reader. `board_id` is the board or the wiki the source is on.
--
-- The rows stay while their source or their target is in the trash and go when either
-- is gone for good, as the versions of a body do: so `source_id` has no key either.
CREATE TABLE body_links (
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    board_id TEXT NOT NULL,
    PRIMARY KEY (source_id, target_id)
);
-- What names a block is asked by the block named.
CREATE INDEX body_links_target_idx ON body_links (target_id);
