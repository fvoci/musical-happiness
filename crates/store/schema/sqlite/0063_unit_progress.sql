-- Personal progress of a learner on each unit of a course: completion timestamp,
-- optional planned date for personal scheduling, and optional last reading or viewing
-- position.
--
-- Rows stay while a unit or its course is in the trash so restoration preserves
-- progress, and are removed when the unit or course is permanently purged (as
-- `body_versions` is): so they carry no foreign key to `blocks` or `containers`.
CREATE TABLE unit_progress (
    -- The unit block (`BlockType::Unit`).
    unit_id TEXT NOT NULL,
    -- The course container holding the unit, indexed for per-course reads and summaries.
    container_id TEXT NOT NULL,
    -- The learner whose personal progress this row records.
    user_id TEXT NOT NULL,
    -- Completion time in milliseconds since the Unix epoch; NULL if not completed.
    completed_at INTEGER,
    -- Optional personal target date (`YYYY-MM-DD`), reserved for personal scheduling.
    planned_date TEXT,
    -- Optional last read/watched position string, at most 256 characters.
    last_position TEXT,
    -- Last modification time in milliseconds since the Unix epoch.
    update_at INTEGER NOT NULL,
    -- Organization identifier for multi-space cleanup on SQLite.
    org_id TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (unit_id, user_id)
);

CREATE INDEX unit_progress_container_user_idx
    ON unit_progress (container_id, user_id);
CREATE INDEX unit_progress_container_completed_idx
    ON unit_progress (container_id, completed_at);
CREATE INDEX unit_progress_user_idx
    ON unit_progress (user_id);
