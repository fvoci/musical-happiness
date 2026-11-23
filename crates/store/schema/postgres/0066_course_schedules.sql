-- Shared study schedule for a course: study period (start_date..=target_date) and
-- selected weekdays bitmask (Mon=1..Sun=64, 1..=127).
--
-- Rows stay while a course is in the trash so restoration preserves the schedule, and
-- are removed when the course is permanently purged (as `unit_progress` is): so they
-- carry no foreign key to `containers`.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy below
-- reads the settings and no row; the line that follows is there because the check of
-- later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE course_schedules (
    -- The course container (`ContainerKind::Course`).
    container_id UUID NOT NULL,
    -- Start of the study period (`YYYY-MM-DD`).
    start_date TEXT NOT NULL,
    -- Target end of the study period (`YYYY-MM-DD`).
    target_date TEXT NOT NULL,
    -- Selected study weekdays bitmask: bit 0 (1) = Monday .. bit 6 (64) = Sunday.
    study_days SMALLINT NOT NULL CHECK (study_days >= 1 AND study_days <= 127),
    -- Last modification time in milliseconds since the Unix epoch.
    update_at BIGINT NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    PRIMARY KEY (container_id, org_id)
);

ALTER TABLE course_schedules ENABLE ROW LEVEL SECURITY;
ALTER TABLE course_schedules FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON course_schedules USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
