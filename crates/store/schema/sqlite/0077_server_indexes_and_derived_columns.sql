-- Composite and partial indexes for block listings, reference autocomplete, writer
-- visibility, and account erasure; derived `deleted_blocks.block_type` and
-- `blocks.body_first_image` columns; `users.email_verified_at`; `course_exams`
-- published result snapshot columns; and course child table integrity triggers.

CREATE INDEX blocks_container_type_created_at_idx
    ON blocks (container_id, type, created_at, id);

DROP INDEX blocks_parent_id_idx;

CREATE INDEX blocks_container_titled_updated_at_idx
    ON blocks (container_id, updated_at DESC, id)
    WHERE type IN ('card', 'page', 'unit');

ALTER TABLE deleted_blocks ADD COLUMN block_type TEXT NOT NULL DEFAULT '';

UPDATE deleted_blocks
SET block_type = COALESCE(
    CASE
        WHEN json_valid(block)
             AND json_type(block) = 'object'
             AND json_type(block, '$.type') = 'text'
            THEN json_extract(block, '$.type')
    END,
    ''
);

DROP INDEX deleted_blocks_container_id_idx;

CREATE INDEX deleted_blocks_container_deleted_at_idx
    ON deleted_blocks (container_id, deleted_at DESC, id);
CREATE INDEX deleted_blocks_container_type_deleted_at_idx
    ON deleted_blocks (container_id, block_type, deleted_at DESC, id);

CREATE INDEX comment_versions_written_by_idx ON comment_versions (written_by);
CREATE INDEX body_version_editors_user_id_idx ON body_version_editors (user_id);
CREATE INDEX files_created_by_idx ON files (created_by);
CREATE INDEX file_versions_created_by_idx ON file_versions (created_by);
CREATE INDEX containers_created_by_idx ON containers (created_by);
CREATE INDEX invitations_created_by_idx ON invitations (created_by);
CREATE INDEX audit_log_actor_identity_idx
    ON audit_log (actor_id) WHERE identity IS NOT NULL;
CREATE INDEX audit_log_subject_identity_idx
    ON audit_log (subject_id) WHERE identity IS NOT NULL;

ALTER TABLE blocks ADD COLUMN body_first_image TEXT;

UPDATE blocks
SET body_first_image = (
    WITH RECURSIVE walk(node, path) AS (
        SELECT elem.value, printf('%08d', elem.key)
        FROM json_each(blocks.body, '$.content') AS elem
        WHERE json_type(blocks.body, '$.content') = 'array'
        UNION ALL
        SELECT child.value, walk.path || '/' || printf('%08d', child.key)
        FROM walk
        JOIN json_each(walk.node, '$.content') AS child
        WHERE json_type(walk.node) = 'object'
          AND COALESCE(json_extract(walk.node, '$.type'), '') <> 'image'
          AND json_type(walk.node, '$.content') = 'array'
    )
    SELECT json_extract(walk.node, '$.attrs.fileId')
    FROM walk
    WHERE json_type(walk.node) = 'object'
      AND json_extract(walk.node, '$.type') = 'image'
      AND json_type(walk.node, '$.attrs.fileId') = 'text'
    ORDER BY walk.path
    LIMIT 1
)
WHERE body IS NOT NULL
  AND instr(body, '"image"') > 0
  AND CASE
      WHEN json_valid(body) THEN json_type(body) = 'object'
      ELSE 0
  END;

ALTER TABLE users ADD COLUMN email_verified_at BIGINT NOT NULL DEFAULT 0;

UPDATE users
SET email_verified_at = created_at
WHERE email <> '' AND erased_at = 0;

ALTER TABLE course_exams ADD COLUMN results_published_at BIGINT;
ALTER TABLE course_exams ADD COLUMN published_average_score REAL;
ALTER TABLE course_exams ADD COLUMN published_score_bucket BIGINT;
ALTER TABLE course_exams ADD COLUMN published_submission_count BIGINT;

UPDATE unit_progress
SET org_id = COALESCE(
    (SELECT w.org_id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id
     WHERE c.id = unit_progress.container_id),
    (SELECT w.org_id FROM deleted_containers AS dc JOIN workspaces AS w ON w.id = dc.workspace_id
     WHERE dc.id = unit_progress.container_id),
    org_id
)
WHERE org_id = '';

UPDATE unit_reviews
SET org_id = COALESCE(
    (SELECT w.org_id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id
     WHERE c.id = unit_reviews.container_id),
    (SELECT w.org_id FROM deleted_containers AS dc JOIN workspaces AS w ON w.id = dc.workspace_id
     WHERE dc.id = unit_reviews.container_id),
    org_id
)
WHERE org_id = '';

UPDATE unit_notes
SET org_id = COALESCE(
    (SELECT w.org_id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id
     WHERE c.id = unit_notes.container_id),
    (SELECT w.org_id FROM deleted_containers AS dc JOIN workspaces AS w ON w.id = dc.workspace_id
     WHERE dc.id = unit_notes.container_id),
    org_id
)
WHERE org_id = '';

UPDATE course_meetings
SET org_id = COALESCE(
    (SELECT w.org_id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id
     WHERE c.id = course_meetings.container_id),
    (SELECT w.org_id FROM deleted_containers AS dc JOIN workspaces AS w ON w.id = dc.workspace_id
     WHERE dc.id = course_meetings.container_id),
    org_id
)
WHERE org_id = '';

UPDATE course_exams
SET org_id = COALESCE(
    (SELECT w.org_id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id
     WHERE c.id = course_exams.container_id),
    (SELECT w.org_id FROM deleted_containers AS dc JOIN workspaces AS w ON w.id = dc.workspace_id
     WHERE dc.id = course_exams.container_id),
    org_id
)
WHERE org_id = '';

UPDATE exam_scores
SET org_id = COALESCE(
    (SELECT w.org_id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id
     WHERE c.id = exam_scores.container_id),
    (SELECT w.org_id FROM deleted_containers AS dc JOIN workspaces AS w ON w.id = dc.workspace_id
     WHERE dc.id = exam_scores.container_id),
    org_id
)
WHERE org_id = '';

-- Rows still left with `org_id = ''` after backfilling point to a course that no longer
-- exists in either `containers` or `deleted_containers`; drop those orphans before
-- installing the course child triggers. `containers.kind` is immutable after creation
-- (`ContainerError::KindChanges` in Rust), so the triggers guard child writes.
DELETE FROM unit_progress WHERE org_id = '';
DELETE FROM unit_reviews WHERE org_id = '';
DELETE FROM unit_notes WHERE org_id = '';
DELETE FROM course_meetings WHERE org_id = '';
DELETE FROM course_exams WHERE org_id = '';
DELETE FROM exam_scores WHERE org_id = '';

CREATE TRIGGER unit_progress_course_of_its_organization
BEFORE INSERT ON unit_progress
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER unit_progress_stays_of_its_course
BEFORE UPDATE OF container_id, org_id ON unit_progress
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER unit_reviews_course_of_its_organization
BEFORE INSERT ON unit_reviews
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER unit_reviews_stays_of_its_course
BEFORE UPDATE OF container_id, org_id ON unit_reviews
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER unit_notes_course_of_its_organization
BEFORE INSERT ON unit_notes
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER unit_notes_stays_of_its_course
BEFORE UPDATE OF container_id, org_id ON unit_notes
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER course_meetings_course_of_its_organization
BEFORE INSERT ON course_meetings
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER course_meetings_stays_of_its_course
BEFORE UPDATE OF container_id, org_id ON course_meetings
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER course_exams_course_of_its_organization
BEFORE INSERT ON course_exams
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER course_exams_stays_of_its_course
BEFORE UPDATE OF container_id, org_id ON course_exams
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER exam_scores_course_of_its_organization
BEFORE INSERT ON exam_scores
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;

CREATE TRIGGER exam_scores_stays_of_its_course
BEFORE UPDATE OF container_id, org_id ON exam_scores
WHEN NOT EXISTS (
    SELECT 1 FROM containers AS c
    JOIN workspaces AS w ON w.id = c.workspace_id
    WHERE c.id = NEW.container_id AND c.kind = 'course' AND w.org_id = NEW.org_id
) AND NOT EXISTS (
    SELECT 1 FROM deleted_containers AS dc
    JOIN workspaces AS w ON w.id = dc.workspace_id
    WHERE dc.id = NEW.container_id AND w.org_id = NEW.org_id
      AND CASE
          WHEN json_valid(dc.container) AND json_type(dc.container) = 'object'
              THEN COALESCE(json_extract(dc.container, '$.kind'), 'board') = 'course'
          ELSE 0
      END
)
BEGIN
    SELECT RAISE(ABORT, 'the course of a course record is not a course of its organization');
END;
