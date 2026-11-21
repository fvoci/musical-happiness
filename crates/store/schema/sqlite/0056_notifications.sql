-- What people are told inside the application, and what they are still to be told.
--
-- A row of `notification_outbox` says that somebody may be told of something: it is
-- written in the transaction that changes a block, and worked off by the server that
-- holds the lock, which asks whether that person reads the block now and only then
-- makes a row of `notifications`. A row of `notifications` holds no title and no
-- name: whoever reads it is asked again, each time, whether they read what it is
-- about.
CREATE TABLE notification_outbox (
    -- The organization of the block when the row was written. Every statement that
    -- writes a row names it, from the row of the block's container.
    org_id TEXT NOT NULL,
    -- Who may be told.
    recipient_id TEXT NOT NULL,
    -- 'mention': a body names them. 'assignment': a person property of a card took
    -- them. 'comment': a card or a page they watch has a new comment.
    kind TEXT NOT NULL CHECK (kind IN ('mention', 'assignment', 'comment')),
    -- Where it happened: the card or the page itself, or a comment on it.
    source_id TEXT NOT NULL,
    -- The card or the page it is about: what the person opens.
    block_id TEXT NOT NULL,
    container_id TEXT NOT NULL,
    -- Who did it; NULL where nobody is known to have.
    actor_id TEXT,
    -- Moved on when the same is noted again before the row was worked off. Who
    -- worked it off takes the row away only if it still has the `seq` they read.
    seq BIGINT NOT NULL,
    -- When the row is next looked at: at once, or later after a failure.
    due_at BIGINT NOT NULL,
    -- How many times in a row it could not be worked off.
    attempts BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, recipient_id, kind, source_id)
);
CREATE INDEX notification_outbox_due_at_idx ON notification_outbox (due_at);

CREATE TABLE notifications (
    id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    -- Whose it is.
    recipient_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('mention', 'assignment', 'comment')),
    -- Where it happened, what it is about and who did it, as in `notification_outbox`.
    -- None of them is a key: a row outlives what it names, and is then about
    -- something its reader cannot see.
    source_id TEXT NOT NULL,
    block_id TEXT NOT NULL,
    container_id TEXT NOT NULL,
    actor_id TEXT,
    create_at BIGINT NOT NULL,
    -- When its reader read it; 0 until then.
    read_at BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, id)
);
-- A person's list, the newest first.
CREATE INDEX notifications_recipient_idx ON notifications (recipient_id, create_at DESC, id DESC);
-- How many of them a person has not read.
CREATE INDEX notifications_unread_idx ON notifications (recipient_id) WHERE read_at = 0;
-- Whether a person was told the same a moment ago.
CREATE INDEX notifications_same_idx ON notifications (recipient_id, source_id, kind, create_at);
