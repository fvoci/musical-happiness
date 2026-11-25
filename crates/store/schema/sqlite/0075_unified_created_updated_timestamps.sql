-- Unified `created_at` and `updated_at` SQL columns across every table and index, plus
-- `createdAt` / `updatedAt` (and `created_at` in `deleted_containers.files`) across
-- `deleted_containers` and `deleted_blocks` JSON snapshots.

DROP INDEX blocks_parent_type_create_at_idx;

ALTER TABLE audit_log RENAME COLUMN create_at TO created_at;
ALTER TABLE blocks RENAME COLUMN create_at TO created_at;
ALTER TABLE blocks RENAME COLUMN update_at TO updated_at;
ALTER TABLE body_docs RENAME COLUMN create_at TO created_at;
ALTER TABLE body_docs RENAME COLUMN update_at TO updated_at;
ALTER TABLE categories RENAME COLUMN create_at TO created_at;
ALTER TABLE categories RENAME COLUMN update_at TO updated_at;
ALTER TABLE category_containers RENAME COLUMN create_at TO created_at;
ALTER TABLE category_containers RENAME COLUMN update_at TO updated_at;
ALTER TABLE containers RENAME COLUMN create_at TO created_at;
ALTER TABLE containers RENAME COLUMN update_at TO updated_at;
ALTER TABLE course_exams RENAME COLUMN create_at TO created_at;
ALTER TABLE course_exams RENAME COLUMN update_at TO updated_at;
ALTER TABLE course_meetings RENAME COLUMN create_at TO created_at;
ALTER TABLE course_meetings RENAME COLUMN update_at TO updated_at;
ALTER TABLE course_schedules RENAME COLUMN update_at TO updated_at;
ALTER TABLE email_changes RENAME COLUMN create_at TO created_at;
ALTER TABLE email_reverts RENAME COLUMN create_at TO created_at;
ALTER TABLE exam_scores RENAME COLUMN update_at TO updated_at;
ALTER TABLE files RENAME COLUMN create_at TO created_at;
ALTER TABLE invitations RENAME COLUMN create_at TO created_at;
ALTER TABLE loose_objects RENAME COLUMN create_at TO created_at;
ALTER TABLE meeting_attendance RENAME COLUMN update_at TO updated_at;
ALTER TABLE notifications RENAME COLUMN create_at TO created_at;
ALTER TABLE notifications RENAME COLUMN update_at TO updated_at;
ALTER TABLE oidc_former_providers RENAME COLUMN create_at TO created_at;
ALTER TABLE oidc_held_sign_ins RENAME COLUMN create_at TO created_at;
ALTER TABLE oidc_login_flows RENAME COLUMN create_at TO created_at;
ALTER TABLE oidc_pending_providers RENAME COLUMN create_at TO created_at;
ALTER TABLE oidc_providers RENAME COLUMN create_at TO created_at;
ALTER TABLE oidc_providers RENAME COLUMN update_at TO updated_at;
ALTER TABLE organization_members RENAME COLUMN create_at TO created_at;
ALTER TABLE organizations RENAME COLUMN create_at TO created_at;
ALTER TABLE organizations RENAME COLUMN update_at TO updated_at;
ALTER TABLE public_links RENAME COLUMN update_at TO updated_at;
ALTER TABLE signups RENAME COLUMN create_at TO created_at;
ALTER TABLE subscriptions RENAME COLUMN create_at TO created_at;
ALTER TABLE time_entries RENAME COLUMN create_at TO created_at;
ALTER TABLE time_entries RENAME COLUMN update_at TO updated_at;
ALTER TABLE unit_notes RENAME COLUMN create_at TO created_at;
ALTER TABLE unit_notes RENAME COLUMN update_at TO updated_at;
ALTER TABLE unit_progress RENAME COLUMN update_at TO updated_at;
ALTER TABLE unit_reviews RENAME COLUMN create_at TO created_at;
ALTER TABLE unit_reviews RENAME COLUMN update_at TO updated_at;
ALTER TABLE uploads RENAME COLUMN create_at TO created_at;
ALTER TABLE user_identities RENAME COLUMN create_at TO created_at;
ALTER TABLE users RENAME COLUMN create_at TO created_at;
ALTER TABLE users RENAME COLUMN update_at TO updated_at;
ALTER TABLE workspace_members RENAME COLUMN create_at TO created_at;
ALTER TABLE workspaces RENAME COLUMN create_at TO created_at;
ALTER TABLE workspaces RENAME COLUMN update_at TO updated_at;

CREATE INDEX blocks_parent_type_created_at_idx ON blocks (parent_id, type, created_at);

DROP TRIGGER audit_log_unchanged;
CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
WHEN NOT (
    NEW.identity IS NULL
    AND NEW.id IS OLD.id AND NEW.org_id IS OLD.org_id
    AND NEW.workspace_id IS OLD.workspace_id AND NEW.actor_id IS OLD.actor_id
    AND NEW.actor_kind IS OLD.actor_kind AND NEW.action IS OLD.action
    AND NEW.target_id IS OLD.target_id AND NEW.subject_id IS OLD.subject_id
    AND NEW.created_at IS OLD.created_at AND NEW.detail IS OLD.detail
    AND EXISTS (SELECT 1 FROM users
                WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0))
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;

UPDATE deleted_containers
SET container = CASE
    WHEN json_type(container, '$.createdAt') IS NULL THEN json_set(
        json_remove(container, '$.createAt'),
        '$.createdAt',
        json(container -> '$.createAt')
    )
    ELSE json_remove(container, '$.createAt')
END
WHERE CASE
    WHEN json_valid(container) THEN
        json_type(container) = 'object'
        AND json_type(container, '$.createAt') IS NOT NULL
    ELSE 0
END;

UPDATE deleted_containers
SET container = CASE
    WHEN json_type(container, '$.updatedAt') IS NULL THEN json_set(
        json_remove(container, '$.updateAt'),
        '$.updatedAt',
        json(container -> '$.updateAt')
    )
    ELSE json_remove(container, '$.updateAt')
END
WHERE CASE
    WHEN json_valid(container) THEN
        json_type(container) = 'object'
        AND json_type(container, '$.updateAt') IS NOT NULL
    ELSE 0
END;

UPDATE deleted_containers
SET files = COALESCE(
    (
        SELECT json_group_array(
            CASE
                WHEN f.type = 'object'
                     AND json_type(f.value, '$.create_at') IS NOT NULL
                    THEN json(
                        CASE
                            WHEN json_type(f.value, '$.created_at') IS NULL THEN json_set(
                                json_remove(f.value, '$.create_at'),
                                '$.created_at',
                                json(f.value -> '$.create_at')
                            )
                            ELSE json_remove(f.value, '$.create_at')
                        END
                    )
                WHEN f.type = 'text' THEN f.value
                WHEN f.type = 'true' THEN json('true')
                WHEN f.type = 'false' THEN json('false')
                WHEN f.type = 'null' THEN NULL
                ELSE json(f.value)
            END
            ORDER BY f.key
        )
        FROM json_each(deleted_containers.files) AS f
    ),
    '[]'
)
WHERE CASE
    WHEN json_valid(deleted_containers.files) THEN
        json_type(deleted_containers.files) = 'array'
        AND EXISTS (
            SELECT 1
            FROM json_each(deleted_containers.files) AS f
            WHERE f.type = 'object'
              AND json_type(f.value, '$.create_at') IS NOT NULL
        )
    ELSE 0
END;

UPDATE deleted_blocks
SET block = CASE
    WHEN json_type(block, '$.createdAt') IS NULL THEN json_set(
        json_remove(block, '$.createAt'),
        '$.createdAt',
        json(block -> '$.createAt')
    )
    ELSE json_remove(block, '$.createAt')
END
WHERE CASE
    WHEN json_valid(block) THEN
        json_type(block) = 'object'
        AND json_type(block, '$.createAt') IS NOT NULL
    ELSE 0
END;

UPDATE deleted_blocks
SET block = CASE
    WHEN json_type(block, '$.updatedAt') IS NULL THEN json_set(
        json_remove(block, '$.updateAt'),
        '$.updatedAt',
        json(block -> '$.updateAt')
    )
    ELSE json_remove(block, '$.updateAt')
END
WHERE CASE
    WHEN json_valid(block) THEN
        json_type(block) = 'object'
        AND json_type(block, '$.updateAt') IS NOT NULL
    ELSE 0
END;

UPDATE deleted_blocks
SET block = json_set(
    block,
    '$.watchedBy',
    json(COALESCE(
        (
            SELECT json_group_array(
                CASE
                    WHEN w.type = 'object'
                         AND json_type(w.value, '$.createAt') IS NOT NULL
                        THEN json(
                            CASE
                                WHEN json_type(w.value, '$.createdAt') IS NULL THEN json_set(
                                    json_remove(w.value, '$.createAt'),
                                    '$.createdAt',
                                    json(w.value -> '$.createAt')
                                )
                                ELSE json_remove(w.value, '$.createAt')
                            END
                        )
                    WHEN w.type = 'text' THEN w.value
                    WHEN w.type = 'true' THEN json('true')
                    WHEN w.type = 'false' THEN json('false')
                    WHEN w.type = 'null' THEN NULL
                    ELSE json(w.value)
                END
                ORDER BY w.key
            )
            FROM json_each(deleted_blocks.block, '$.watchedBy') AS w
        ),
        '[]'
    ))
)
WHERE CASE
    WHEN json_valid(deleted_blocks.block) THEN
        json_type(deleted_blocks.block) = 'object'
        AND json_type(deleted_blocks.block, '$.watchedBy') = 'array'
        AND EXISTS (
            SELECT 1
            FROM json_each(deleted_blocks.block, '$.watchedBy') AS w
            WHERE w.type = 'object'
              AND json_type(w.value, '$.createAt') IS NOT NULL
        )
    ELSE 0
END;
