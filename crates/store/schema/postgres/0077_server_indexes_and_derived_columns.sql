-- Composite and partial indexes for block listings, reference autocomplete, writer
-- visibility, and account erasure; derived `deleted_blocks.block_type` and
-- `blocks.body_first_image` columns; PostgreSQL `containers.visibility` default
-- alignment; `users.email_verified_at`; `course_exams` published result snapshot
-- columns; and course child table integrity triggers.
--
-- `blocks` and `deleted_blocks` are behind the organization wall, so the system scope
-- is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

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
        WHEN json_typeof(block) = 'object'
             AND json_typeof(block -> 'type') = 'string'
            THEN block ->> 'type'
    END,
    ''
)
WHERE block::text !~ '\\u0000';

DO $$
DECLARE
    r RECORD;
    doc JSONB;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
BEGIN
    FOR r IN
        SELECT id, org_id, block::text AS raw
        FROM deleted_blocks
        WHERE block::text ~ '\\u0000'
    LOOP
        bs_sentinel := '__FVOCI_BS_' || md5(r.raw) || '__';
        nul_sentinel := '__FVOCI_NUL_' || md5(r.raw) || '__';
        safe := replace(r.raw, '\\', bs_sentinel);
        safe := replace(safe, '\u0000', nul_sentinel);
        safe := replace(safe, bs_sentinel, '\\');
        BEGIN
            doc := safe::jsonb;
        EXCEPTION WHEN invalid_text_representation OR untranslatable_character THEN
            RAISE WARNING '0077: skipped unreadable deleted_blocks.block row %', r.id;
            CONTINUE;
        END;

        IF jsonb_typeof(doc) = 'object' AND jsonb_typeof(doc -> 'type') = 'string' THEN
            UPDATE deleted_blocks
            SET block_type = replace(doc ->> 'type', nul_sentinel, '')
            WHERE id = r.id AND org_id = r.org_id;
        END IF;
    END LOOP;
END $$;

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

UPDATE blocks AS b
SET body_first_image = (
    WITH RECURSIVE walk(node, path) AS (
        SELECT elem.val, ARRAY[elem.ord]
        FROM jsonb_array_elements(
            CASE
                WHEN jsonb_typeof(b.body -> 'content') = 'array'
                    THEN b.body -> 'content'
                ELSE '[]'::jsonb
            END
        ) WITH ORDINALITY AS elem(val, ord)
        UNION ALL
        SELECT child.val, walk.path || child.ord
        FROM walk
        CROSS JOIN LATERAL jsonb_array_elements(
            CASE
                WHEN jsonb_typeof(walk.node) = 'object'
                     AND walk.node ->> 'type' IS DISTINCT FROM 'image'
                     AND jsonb_typeof(walk.node -> 'content') = 'array'
                    THEN walk.node -> 'content'
                ELSE '[]'::jsonb
            END
        ) WITH ORDINALITY AS child(val, ord)
    )
    SELECT walk.node -> 'attrs' ->> 'fileId'
    FROM walk
    WHERE jsonb_typeof(walk.node) = 'object'
      AND walk.node ->> 'type' = 'image'
      AND jsonb_typeof(walk.node -> 'attrs' -> 'fileId') = 'string'
    ORDER BY walk.path
    LIMIT 1
)
WHERE b.body IS NOT NULL
  AND jsonb_typeof(b.body) = 'object'
  AND b.body::text LIKE '%"image"%';

ALTER TABLE containers ALTER COLUMN visibility SET DEFAULT 'private';

ALTER TABLE users ADD COLUMN email_verified_at BIGINT NOT NULL DEFAULT 0;

UPDATE users
SET email_verified_at = created_at
WHERE email <> '' AND erased_at = 0;

ALTER TABLE course_exams ADD COLUMN results_published_at BIGINT;
ALTER TABLE course_exams ADD COLUMN published_average_score DOUBLE PRECISION;
ALTER TABLE course_exams ADD COLUMN published_score_bucket INTEGER;
ALTER TABLE course_exams ADD COLUMN published_submission_count INTEGER;

CREATE FUNCTION course_child_of_its_course() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    trashed_raw TEXT;
    is_trashed_course BOOLEAN := false;
BEGIN
    IF NEW.org_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF EXISTS (
        SELECT 1 FROM containers
        WHERE id = NEW.container_id AND org_id = NEW.org_id AND kind = 'course'
    ) THEN
        RETURN NEW;
    END IF;
    SELECT container::text INTO trashed_raw
    FROM deleted_containers
    WHERE id = NEW.container_id AND org_id = NEW.org_id;
    IF FOUND THEN
        BEGIN
            is_trashed_course := COALESCE((trashed_raw::json) ->> 'kind', 'board') = 'course';
        EXCEPTION WHEN untranslatable_character OR invalid_text_representation THEN
            is_trashed_course := trashed_raw ~ '"kind"\s*:\s*"course"';
        END;
    END IF;
    IF NOT is_trashed_course THEN
        RAISE foreign_key_violation USING
            MESSAGE = 'the course of a course record is not a course of its organization';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER unit_progress_course_of_its_organization
    BEFORE INSERT OR UPDATE OF container_id, org_id ON unit_progress
    FOR EACH ROW EXECUTE FUNCTION course_child_of_its_course();

CREATE TRIGGER unit_reviews_course_of_its_organization
    BEFORE INSERT OR UPDATE OF container_id, org_id ON unit_reviews
    FOR EACH ROW EXECUTE FUNCTION course_child_of_its_course();

CREATE TRIGGER unit_notes_course_of_its_organization
    BEFORE INSERT OR UPDATE OF container_id, org_id ON unit_notes
    FOR EACH ROW EXECUTE FUNCTION course_child_of_its_course();

CREATE TRIGGER course_meetings_course_of_its_organization
    BEFORE INSERT OR UPDATE OF container_id, org_id ON course_meetings
    FOR EACH ROW EXECUTE FUNCTION course_child_of_its_course();

CREATE TRIGGER course_exams_course_of_its_organization
    BEFORE INSERT OR UPDATE OF container_id, org_id ON course_exams
    FOR EACH ROW EXECUTE FUNCTION course_child_of_its_course();

CREATE TRIGGER exam_scores_course_of_its_organization
    BEFORE INSERT OR UPDATE OF container_id, org_id ON exam_scores
    FOR EACH ROW EXECUTE FUNCTION course_child_of_its_course();
