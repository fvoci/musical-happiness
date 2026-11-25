-- Nullable `deleted_at` (`NULL` while in use, positive epoch milliseconds once deleted
-- or waiting to be erased) across `organizations`, `workspaces`, and `users`, plus
-- `deletedAt` (omitted while live) in `deleted_containers` and `deleted_blocks` JSON.

ALTER TABLE organizations ADD COLUMN deleted_at BIGINT
    CHECK (deleted_at IS NULL OR deleted_at > 0);
UPDATE organizations
SET deleted_at = CASE WHEN delete_at > 0 THEN delete_at ELSE NULL END;
ALTER TABLE organizations DROP COLUMN delete_at;

DROP INDEX workspaces_name_in_use_key;
DROP INDEX workspaces_deleted_idx;
ALTER TABLE workspaces ADD COLUMN deleted_at BIGINT
    CHECK (deleted_at IS NULL OR deleted_at > 0);
UPDATE workspaces
SET deleted_at = CASE WHEN delete_at > 0 THEN delete_at ELSE NULL END;
ALTER TABLE workspaces DROP COLUMN delete_at;
CREATE UNIQUE INDEX workspaces_name_in_use_key ON workspaces (org_id, name)
    WHERE deleted_at IS NULL AND name <> '';
CREATE INDEX workspaces_deleted_idx ON workspaces (deleted_at)
    WHERE deleted_at IS NOT NULL;

-- SQLite re-parses triggers on `DROP COLUMN`; `audit_log_unchanged` names
-- `users.erase_started_at`, so drop and recreate it around the `users` column rebuild.
DROP INDEX users_leaving_idx;
DROP TRIGGER audit_log_unchanged;

ALTER TABLE users ADD COLUMN deleted_at BIGINT
    CHECK (deleted_at IS NULL OR deleted_at > 0);
UPDATE users
SET deleted_at = CASE WHEN delete_at > 0 THEN delete_at ELSE NULL END;

ALTER TABLE users ADD COLUMN delete_after_new BIGINT NOT NULL DEFAULT 0
    CHECK (delete_after_new = 0 OR (deleted_at IS NOT NULL AND deleted_at > 0));
ALTER TABLE users ADD COLUMN erase_started_at_new BIGINT NOT NULL DEFAULT 0
    CHECK (erase_started_at_new = 0 OR (deleted_at IS NOT NULL AND deleted_at > 0));
ALTER TABLE users ADD COLUMN erased_at_new BIGINT NOT NULL DEFAULT 0
    CHECK (erased_at_new = 0 OR erase_started_at_new > 0);
UPDATE users
SET delete_after_new = delete_after,
    erase_started_at_new = erase_started_at,
    erased_at_new = erased_at;

ALTER TABLE users DROP COLUMN erased_at;
ALTER TABLE users DROP COLUMN erase_started_at;
ALTER TABLE users DROP COLUMN delete_after;
ALTER TABLE users DROP COLUMN delete_at;

ALTER TABLE users RENAME COLUMN delete_after_new TO delete_after;
ALTER TABLE users RENAME COLUMN erase_started_at_new TO erase_started_at;
ALTER TABLE users RENAME COLUMN erased_at_new TO erased_at;

CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
WHEN NOT (
    NEW.identity IS NULL
    AND NEW.id IS OLD.id AND NEW.org_id IS OLD.org_id
    AND NEW.workspace_id IS OLD.workspace_id AND NEW.actor_id IS OLD.actor_id
    AND NEW.actor_kind IS OLD.actor_kind AND NEW.action IS OLD.action
    AND NEW.target_id IS OLD.target_id AND NEW.subject_id IS OLD.subject_id
    AND NEW.create_at IS OLD.create_at AND NEW.detail IS OLD.detail
    AND EXISTS (SELECT 1 FROM users
                WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0))
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;

CREATE INDEX users_leaving_idx ON users (delete_after)
    WHERE deleted_at IS NOT NULL AND erased_at = 0;

UPDATE deleted_containers
SET container = CASE
    WHEN json_type(container, '$.deletedAt') IS NULL
         AND json_type(container, '$.deleteAt') IN ('integer', 'real')
         AND json_extract(container, '$.deleteAt') > 0
        THEN json_set(
            json_remove(container, '$.deleteAt'),
            '$.deletedAt',
            json(container -> '$.deleteAt')
        )
    WHEN json_type(container, '$.deletedAt') = 'null'
         OR (
             json_type(container, '$.deletedAt') IN ('integer', 'real')
             AND json_extract(container, '$.deletedAt') <= 0
         )
        THEN json_remove(container, '$.deleteAt', '$.deletedAt')
    ELSE json_remove(container, '$.deleteAt')
END
WHERE json_type(container) = 'object'
  AND (
      json_type(container, '$.deleteAt') IS NOT NULL
      OR json_type(container, '$.deletedAt') = 'null'
      OR (
          json_type(container, '$.deletedAt') IN ('integer', 'real')
          AND json_extract(container, '$.deletedAt') <= 0
      )
  );

UPDATE deleted_blocks
SET block = CASE
    WHEN json_type(block, '$.deletedAt') IS NULL
         AND json_type(block, '$.deleteAt') IN ('integer', 'real')
         AND json_extract(block, '$.deleteAt') > 0
        THEN json_set(
            json_remove(block, '$.deleteAt'),
            '$.deletedAt',
            json(block -> '$.deleteAt')
        )
    WHEN json_type(block, '$.deletedAt') = 'null'
         OR (
             json_type(block, '$.deletedAt') IN ('integer', 'real')
             AND json_extract(block, '$.deletedAt') <= 0
         )
        THEN json_remove(block, '$.deleteAt', '$.deletedAt')
    ELSE json_remove(block, '$.deleteAt')
END
WHERE json_type(block) = 'object'
  AND (
      json_type(block, '$.deleteAt') IS NOT NULL
      OR json_type(block, '$.deletedAt') = 'null'
      OR (
          json_type(block, '$.deletedAt') IN ('integer', 'real')
          AND json_extract(block, '$.deletedAt') <= 0
      )
  );
