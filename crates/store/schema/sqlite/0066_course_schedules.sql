-- Shared study schedule for a course: study period (start_date..=target_date) and
-- selected weekdays bitmask (Mon=1..Sun=64, 1..=127).
--
-- Rows stay while a course is in the trash so restoration preserves the schedule, and
-- are removed when the course is permanently purged (as `unit_progress` is): so they
-- carry no foreign key to `containers`.
CREATE TABLE course_schedules (
    -- The course container (`ContainerKind::Course`).
    container_id TEXT NOT NULL,
    -- Start of the study period (`YYYY-MM-DD`).
    start_date TEXT NOT NULL,
    -- Target end of the study period (`YYYY-MM-DD`).
    target_date TEXT NOT NULL,
    -- Selected study weekdays bitmask: bit 0 (1) = Monday .. bit 6 (64) = Sunday.
    study_days INTEGER NOT NULL CHECK (study_days >= 1 AND study_days <= 127),
    -- Last modification time in milliseconds since the Unix epoch.
    update_at INTEGER NOT NULL,
    -- Organization identifier for multi-space parity on SQLite.
    org_id TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (container_id, org_id)
);
