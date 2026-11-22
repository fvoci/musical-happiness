-- A notification is one line for all that happened to one card or page, of one kind,
-- for its reader, within an hour of the line's first: the line counts how many times
-- and by how many, and points to the latest of it.
--
-- Who made the line: the first who did what it tells of. The most lines one person
-- makes for another in an hour are counted by it.
ALTER TABLE notifications ADD COLUMN made_by TEXT;
-- When the latest of it happened; the lines of a reader are listed by it.
ALTER TABLE notifications ADD COLUMN update_at BIGINT NOT NULL DEFAULT 0;
-- How many times it happened.
ALTER TABLE notifications ADD COLUMN times BIGINT NOT NULL DEFAULT 1;
-- Who did it, each once, in the order they first did: a JSON list of identifiers,
-- of a hundred at most.
ALTER TABLE notifications ADD COLUMN doers TEXT NOT NULL DEFAULT '[]';
UPDATE notifications SET made_by = actor_id, update_at = create_at,
    doers = CASE WHEN actor_id IS NULL THEN '[]' ELSE '["' || actor_id || '"]' END;

DROP INDEX notifications_recipient_idx;
CREATE INDEX notifications_recipient_idx ON notifications (recipient_id, update_at DESC, id DESC);
-- The line a reader has of a card or a page, of a kind, made within the hour.
DROP INDEX notifications_same_idx;
CREATE INDEX notifications_same_idx ON notifications (recipient_id, block_id, kind, create_at);
-- How many lines one person made for another within the hour.
CREATE INDEX notifications_made_by_idx ON notifications (recipient_id, made_by, create_at);
-- What goes with a card, a page, a board or a wiki that is deleted for good.
CREATE INDEX notifications_block_idx ON notifications (block_id);
CREATE INDEX notifications_container_idx ON notifications (container_id);
CREATE INDEX notification_outbox_block_idx ON notification_outbox (block_id);
CREATE INDEX notification_outbox_container_idx ON notification_outbox (container_id);
