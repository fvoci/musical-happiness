-- The body of a card or a comment: one document of the editor, checked by the server
-- against its allowlist before it is stored (docs/s6-design.md 3.7, 5.7). NULL for a
-- block without one; an empty document is stored as NULL too.
ALTER TABLE blocks ADD COLUMN body TEXT;
-- The version of the allowlist the body was checked against; NULL without a body.
ALTER TABLE blocks ADD COLUMN body_version BIGINT;
-- The text of the body, a line per paragraph, for searching; NULL without a body.
ALTER TABLE blocks ADD COLUMN body_text TEXT;
-- The task items of the body, and how many are checked: a list of cards shows them
-- without reading the bodies.
ALTER TABLE blocks ADD COLUMN body_tasks_total BIGINT NOT NULL DEFAULT 0;
ALTER TABLE blocks ADD COLUMN body_tasks_done BIGINT NOT NULL DEFAULT 0;
