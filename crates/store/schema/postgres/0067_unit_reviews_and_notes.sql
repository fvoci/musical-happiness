-- Personal spaced-repetition review schedules (`unit_reviews`), personal study notes
-- (`unit_notes`), and the `review_due` notification kind.
--
-- Rows stay while a unit or its course is in the trash so restoration preserves personal
-- reviews and notes, and are removed when the unit or course is permanently purged (as
-- `body_versions` and `unit_progress` are): so they carry no foreign key to `blocks` or
-- `containers`.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policies below
-- read the settings and no row; the line that follows is there because the check of
-- later migrations sees the word of their subqueries (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE unit_reviews (
    -- Unique review identifier.
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    -- The unit block (`BlockType::Unit`).
    unit_id UUID NOT NULL,
    -- The course container holding the unit.
    container_id UUID NOT NULL,
    -- The learner who owns this review schedule.
    user_id UUID NOT NULL,
    -- Scheduled calendar day (`YYYY-MM-DD`) in the learner's calendar.
    scheduled_date TEXT NOT NULL,
    -- Completion time in milliseconds since the Unix epoch; NULL while pending.
    completed_at BIGINT,
    -- Review interval in days (1..=3650).
    interval_days INTEGER NOT NULL CHECK (interval_days >= 1 AND interval_days <= 3650),
    -- Timestamp when the due notification was created; NULL until notified.
    notified_at BIGINT,
    -- Creation time in milliseconds since the Unix epoch.
    create_at BIGINT NOT NULL,
    -- Last modification time in milliseconds since the Unix epoch.
    update_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE INDEX unit_reviews_user_scheduled_idx
    ON unit_reviews (org_id, user_id, scheduled_date);
CREATE INDEX unit_reviews_container_user_unit_idx
    ON unit_reviews (org_id, container_id, user_id, unit_id);
CREATE INDEX unit_reviews_due_notify_idx
    ON unit_reviews (scheduled_date, create_at, id)
    WHERE completed_at IS NULL AND notified_at IS NULL;
CREATE INDEX unit_reviews_user_idx
    ON unit_reviews (user_id);

ALTER TABLE unit_reviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE unit_reviews FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON unit_reviews USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

CREATE TABLE unit_notes (
    -- Unique note identifier.
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    -- The unit block (`BlockType::Unit`).
    unit_id UUID NOT NULL,
    -- The course container holding the unit.
    container_id UUID NOT NULL,
    -- The learner who owns this personal note.
    user_id UUID NOT NULL,
    -- Category of study note: question, mistake, or review.
    kind TEXT NOT NULL CHECK (kind IN ('question', 'mistake', 'review')),
    -- Note text (trimmed non-empty, up to 10,000 characters).
    content TEXT NOT NULL,
    -- Creation time in milliseconds since the Unix epoch.
    create_at BIGINT NOT NULL,
    -- Last modification time in milliseconds since the Unix epoch.
    update_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE INDEX unit_notes_container_user_unit_created_idx
    ON unit_notes (org_id, container_id, user_id, unit_id, create_at DESC);
CREATE INDEX unit_notes_user_idx
    ON unit_notes (user_id);

ALTER TABLE unit_notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE unit_notes FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON unit_notes USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Widen `notifications.kind` to allow personal `review_due` reminders.
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('mention', 'assignment', 'comment', 'review_due'));
