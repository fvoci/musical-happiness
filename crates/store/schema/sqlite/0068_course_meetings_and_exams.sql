-- Course study meetings, attendance records, exams, and strictly personal exam scores.
--
-- Rows stay while a course is in the trash so restoration preserves meetings, attendance,
-- exams, and scores, and are removed when the course is permanently purged (as
-- `unit_progress` and `body_versions` are): so they carry no foreign key to `containers`.
CREATE TABLE course_meetings (
    id TEXT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    container_id TEXT NOT NULL,
    title TEXT NOT NULL,
    starts_at BIGINT NOT NULL,
    location TEXT NOT NULL DEFAULT '',
    unit_ids TEXT NOT NULL DEFAULT '[]',
    presenter_id TEXT,
    host_id TEXT,
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

-- Attendance records are shared across the course and intentionally kept as tombstones
-- (pointing to the erased user's id) on account deletion.
CREATE TABLE meeting_attendance (
    meeting_id TEXT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    user_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('present', 'late', 'absent', 'excused')),
    update_at BIGINT NOT NULL,
    PRIMARY KEY (meeting_id, org_id, user_id)
);

CREATE INDEX meeting_attendance_meeting_idx
    ON meeting_attendance (org_id, meeting_id);
CREATE INDEX meeting_attendance_user_idx
    ON meeting_attendance (user_id);

CREATE TABLE course_exams (
    id TEXT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    container_id TEXT NOT NULL,
    title TEXT NOT NULL,
    exam_date TEXT,
    max_score REAL NOT NULL CHECK (max_score > 0 AND max_score <= 1000000000),
    description TEXT NOT NULL DEFAULT '',
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE INDEX course_exams_container_idx
    ON course_exams (org_id, container_id, exam_date, create_at, id);

-- Strictly personal exam scores: only the owner ever sees their own score or note;
-- other course members see only aggregate averages and counts over active members.
CREATE TABLE exam_scores (
    exam_id TEXT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    container_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    score REAL NOT NULL CHECK (score >= 0 AND score <= 1000000000),
    note TEXT NOT NULL DEFAULT '',
    update_at BIGINT NOT NULL,
    PRIMARY KEY (exam_id, org_id, user_id)
);

CREATE INDEX exam_scores_container_user_idx
    ON exam_scores (org_id, container_id, user_id);
CREATE INDEX exam_scores_user_idx
    ON exam_scores (user_id);

-- Widen `notifications.kind` in place without rebuilding the table, as 0061 did.
DROP INDEX notifications_same_idx;
ALTER TABLE notifications ADD COLUMN kind_wider TEXT NOT NULL DEFAULT 'mention'
    CHECK (kind_wider IN ('mention', 'assignment', 'comment', 'review_due', 'meeting_upcoming'));
UPDATE notifications SET kind_wider = kind;
ALTER TABLE notifications DROP COLUMN kind;
ALTER TABLE notifications RENAME COLUMN kind_wider TO kind;
CREATE INDEX notifications_same_idx
    ON notifications (recipient_id, block_id, kind, create_at);
