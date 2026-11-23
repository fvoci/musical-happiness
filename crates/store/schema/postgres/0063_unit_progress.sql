-- Personal progress of a learner on each unit of a course: completion timestamp,
-- optional planned date for personal scheduling, and optional last reading or viewing
-- position.
--
-- Rows stay while a unit or its course is in the trash so restoration preserves
-- progress, and are removed when the unit or course is permanently purged (as
-- `body_versions` is): so they carry no foreign key to `blocks` or `containers`.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy below
-- reads the settings and no row; the line that follows is there because the check of
-- later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE unit_progress (
    -- The unit block (`BlockType::Unit`).
    unit_id UUID NOT NULL,
    -- The course container holding the unit, indexed for per-course reads and summaries.
    container_id UUID NOT NULL,
    -- The learner whose personal progress this row records.
    user_id UUID NOT NULL,
    -- Completion time in milliseconds since the Unix epoch; NULL if not completed.
    completed_at BIGINT,
    -- Optional personal target date (`YYYY-MM-DD`), reserved for personal scheduling.
    planned_date TEXT,
    -- Optional last read/watched position string, at most 256 characters.
    last_position TEXT,
    -- Last modification time in milliseconds since the Unix epoch.
    update_at BIGINT NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    PRIMARY KEY (unit_id, org_id, user_id)
);

CREATE INDEX unit_progress_container_user_idx
    ON unit_progress (org_id, container_id, user_id);
CREATE INDEX unit_progress_container_completed_idx
    ON unit_progress (org_id, container_id, completed_at);
CREATE INDEX unit_progress_user_idx
    ON unit_progress (user_id);

ALTER TABLE unit_progress ENABLE ROW LEVEL SECURITY;
ALTER TABLE unit_progress FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON unit_progress USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
