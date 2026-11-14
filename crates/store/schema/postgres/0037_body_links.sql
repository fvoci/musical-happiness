-- Which block's body names which block: a row per reference node of the body a block
-- holds now (`reference`, whose `id` is the block named), put down again whenever that
-- body is written. `target_id` has no key to a block: a body may name a block that is
-- not there, one of another workspace, one its writer cannot see, and the row says no
-- more than the body does. Who may read the source and the target is asked when the
-- rows are read, of each reader. `board_id` is the board or the wiki the source is on.
--
-- The rows stay while their source or their target is in the trash and go when either
-- is gone for good, as the versions of a body do: so `source_id` has no key either.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy
-- below reads the settings and no row; the line that follows is there because the check
-- of later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE body_links (
    source_id UUID NOT NULL,
    target_id UUID NOT NULL,
    board_id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    PRIMARY KEY (source_id, org_id, target_id)
);
-- What names a block is asked by the block named.
CREATE INDEX body_links_target_idx ON body_links (target_id, org_id);

ALTER TABLE body_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE body_links FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON body_links USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
