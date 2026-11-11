-- The content blocks that were stored before a card's content became its body (text,
-- image, checkbox, divider, headings and every other type that is not a card, a view, a
-- comment, an attachment or the legacy board block): deleted, live and in the trash, by the server once after
-- the migrations (crates/store/src/data_steps.rs), and their files left to be collected.
INSERT INTO data_steps (name) VALUES ('content_blocks');
