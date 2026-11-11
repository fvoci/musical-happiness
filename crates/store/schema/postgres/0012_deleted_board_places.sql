-- Where each user had filed a deleted board (category, sort order, hidden), so that
-- restoring the board puts it back there. Goes with the board's row when it is purged.
ALTER TABLE deleted_boards ADD COLUMN places JSON NOT NULL DEFAULT '[]';
