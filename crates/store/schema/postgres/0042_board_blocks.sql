-- The blocks of the type `board`, the original's board block, which a write of blocks
-- took before only the current types were taken and which the step of migration 8
-- kept: deleted with every block below them, live and in the trash, by the server once
-- after the migrations (crates/store/src/data_steps.rs). No block of that type is made
-- any more.
INSERT INTO data_steps (name) VALUES ('board_blocks');
