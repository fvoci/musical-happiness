-- Course study meetings, attendance records, exams, and strictly personal exam scores.
--
-- Rows stay while a course is in the trash so restoration preserves meetings, attendance,
-- exams, and scores, and are removed when the course is permanently purged (as
-- `unit_progress` and `body_versions` are): so they carry no foreign key to `containers`.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy below
-- reads the settings and no row; the line that follows is there because the check of
-- later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE course_meetings (
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    container_id UUID NOT NULL,
    title TEXT NOT NULL,
    starts_at BIGINT NOT NULL,
    location TEXT NOT NULL DEFAULT '',
    unit_ids TEXT NOT NULL DEFAULT '[]',
    presenter_id UUID,
    host_id UUID,
    notes TEXT NOT NULL DEFAULT '',
    notified_at BIGINT,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE INDEX course_meetings_container_idx
    ON course_meetings (org_id, container_id, starts_at, id);
CREATE INDEX course_meetings_upcoming_idx
    ON course_meetings (starts_at) WHERE notified_at IS NULL;

ALTER TABLE course_meetings ENABLE ROW LEVEL SECURITY;
ALTER TABLE course_meetings FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON course_meetings USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Attendance records are shared across the course and intentionally kept as tombstones
-- (pointing to the erased user's id) on account deletion.
CREATE TABLE meeting_attendance (
    meeting_id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    user_id UUID NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('present', 'late', 'absent', 'excused')),
    update_at BIGINT NOT NULL,
    PRIMARY KEY (meeting_id, org_id, user_id)
);

CREATE INDEX meeting_attendance_meeting_idx
    ON meeting_attendance (org_id, meeting_id);
CREATE INDEX meeting_attendance_user_idx
    ON meeting_attendance (user_id);

ALTER TABLE meeting_attendance ENABLE ROW LEVEL SECURITY;
ALTER TABLE meeting_attendance FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON meeting_attendance USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

CREATE TABLE course_exams (
    id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    container_id UUID NOT NULL,
    title TEXT NOT NULL,
    exam_date TEXT,
    max_score DOUBLE PRECISION NOT NULL CHECK (max_score > 0 AND max_score <= 1000000000),
    description TEXT NOT NULL DEFAULT '',
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE INDEX course_exams_container_idx
    ON course_exams (org_id, container_id, exam_date, create_at, id);

ALTER TABLE course_exams ENABLE ROW LEVEL SECURITY;
ALTER TABLE course_exams FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON course_exams USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Strictly personal exam scores: only the owner ever sees their own score or note;
-- other course members see only aggregate averages and counts over active members.
CREATE TABLE exam_scores (
    exam_id UUID NOT NULL,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    container_id UUID NOT NULL,
    user_id UUID NOT NULL,
    score DOUBLE PRECISION NOT NULL CHECK (score >= 0 AND score <= 1000000000),
    note TEXT NOT NULL DEFAULT '',
    update_at BIGINT NOT NULL,
    PRIMARY KEY (exam_id, org_id, user_id)
);

CREATE INDEX exam_scores_container_user_idx
    ON exam_scores (org_id, container_id, user_id);
CREATE INDEX exam_scores_user_idx
    ON exam_scores (user_id);

ALTER TABLE exam_scores ENABLE ROW LEVEL SECURITY;
ALTER TABLE exam_scores FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON exam_scores USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Allow upcoming course meeting reminders in `notifications`.
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('mention', 'assignment', 'comment', 'meeting_upcoming'));
