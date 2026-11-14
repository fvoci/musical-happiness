-- What a board is for: 'board' holds cards, 'wiki' holds pages. Every board that was
-- there before is a board. The kind is what the board was made as and is never
-- changed: the server does not write the column again.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE boards ADD COLUMN kind TEXT NOT NULL DEFAULT 'board' CHECK (kind IN ('board', 'wiki'));
