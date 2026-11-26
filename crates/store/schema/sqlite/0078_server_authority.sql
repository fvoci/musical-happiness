-- Server-authority schema additions: `workspaces.welcomed_at`,
-- `organizations.allow_copy_out`, board completion and due-date metadata
-- (`containers.due_date_property_id`, `containers.done_when`), derived card due/done
-- columns (`blocks.due_at`, `blocks.due_millis`, `blocks.is_done`), `card_assignees`,
-- `card_due_notices`, and widened audit/notification kinds.

ALTER TABLE workspaces ADD COLUMN welcomed_at BIGINT
    CHECK (welcomed_at IS NULL OR welcomed_at > 0);

UPDATE workspaces
SET welcomed_at = CASE WHEN created_at > 0 THEN created_at ELSE 1 END
WHERE is_system = FALSE
  AND (
      EXISTS (SELECT 1 FROM containers AS c WHERE c.workspace_id = workspaces.id)
      OR EXISTS (SELECT 1 FROM deleted_containers AS dc WHERE dc.workspace_id = workspaces.id)
      OR EXISTS (
          SELECT 1 FROM workspace_members AS wm
          JOIN users AS u ON u.id = wm.user_id
          WHERE wm.workspace_id = workspaces.id AND u.welcomed_at IS NOT NULL
      )
  );

ALTER TABLE organizations ADD COLUMN allow_copy_out BOOLEAN NOT NULL DEFAULT TRUE;

UPDATE organizations
SET allow_copy_out = FALSE
WHERE EXISTS (
    SELECT 1 FROM oidc_providers AS p
    WHERE p.org_id = organizations.id AND p.sso_required = TRUE
);

ALTER TABLE containers ADD COLUMN due_date_property_id TEXT;
ALTER TABLE containers ADD COLUMN done_when TEXT;

ALTER TABLE blocks ADD COLUMN due_at TEXT;
ALTER TABLE blocks ADD COLUMN due_millis BIGINT;
ALTER TABLE blocks ADD COLUMN is_done BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX blocks_due_at_idx
    ON blocks (container_id, due_at)
    WHERE due_at IS NOT NULL AND is_done = FALSE;

CREATE TABLE card_assignees (
    org_id TEXT NOT NULL DEFAULT '',
    container_id TEXT NOT NULL REFERENCES containers (id) ON DELETE CASCADE,
    card_id TEXT NOT NULL REFERENCES blocks (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (org_id, card_id, user_id)
);

CREATE INDEX card_assignees_user_container_idx
    ON card_assignees (org_id, user_id, container_id);
CREATE INDEX card_assignees_container_idx
    ON card_assignees (container_id);
CREATE INDEX card_assignees_user_idx
    ON card_assignees (user_id);

CREATE TABLE card_due_notices (
    org_id TEXT NOT NULL DEFAULT '',
    card_id TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    due_key TEXT NOT NULL,
    notified_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, card_id, user_id, due_key)
);

CREATE INDEX card_due_notices_user_idx
    ON card_due_notices (user_id);
CREATE INDEX card_due_notices_card_idx
    ON card_due_notices (card_id);

-- Backfill `card_assignees` for existing live non-template cards on boards.
INSERT OR IGNORE INTO card_assignees (org_id, container_id, card_id, user_id)
SELECT w.org_id, c.id, b.id, u.id
FROM blocks AS b
JOIN containers AS c ON c.id = b.container_id AND c.kind = 'board' AND c.is_template = FALSE
JOIN workspaces AS w ON w.id = c.workspace_id
JOIN json_each(CASE WHEN json_valid(c.card_properties) AND json_type(c.card_properties) = 'array' THEN c.card_properties ELSE '[]' END) AS prop
    ON json_type(prop.value) = 'object'
   AND json_extract(prop.value, '$.type') = 'person'
   AND json_type(prop.value, '$.id') = 'text'
JOIN json_each(CASE WHEN json_valid(b.fields) AND json_type(b.fields, '$.properties') = 'object' THEN json_extract(b.fields, '$.properties') ELSE '{}' END) AS kv
    ON kv.key = json_extract(prop.value, '$.id')
   AND kv.type = 'text'
JOIN users AS u ON u.id = kv.value
WHERE b.type = 'card'
  AND CASE WHEN json_valid(b.fields) THEN COALESCE(json_type(b.fields, '$.isTemplate'), '') <> 'true' ELSE 1 END;

INSERT OR IGNORE INTO card_assignees (org_id, container_id, card_id, user_id)
SELECT w.org_id, c.id, b.id, u.id
FROM blocks AS b
JOIN containers AS c ON c.id = b.container_id AND c.kind = 'board' AND c.is_template = FALSE
JOIN workspaces AS w ON w.id = c.workspace_id
JOIN json_each(CASE WHEN json_valid(c.card_properties) AND json_type(c.card_properties) = 'array' THEN c.card_properties ELSE '[]' END) AS prop
    ON json_type(prop.value) = 'object'
   AND json_extract(prop.value, '$.type') = 'multiPerson'
   AND json_type(prop.value, '$.id') = 'text'
JOIN json_each(CASE WHEN json_valid(b.fields) AND json_type(b.fields, '$.properties') = 'object' THEN json_extract(b.fields, '$.properties') ELSE '{}' END) AS kv
    ON kv.key = json_extract(prop.value, '$.id')
   AND kv.type = 'array'
JOIN json_each(kv.value) AS item
    ON item.type = 'text'
JOIN users AS u ON u.id = item.value
WHERE b.type = 'card'
  AND CASE WHEN json_valid(b.fields) THEN COALESCE(json_type(b.fields, '$.isTemplate'), '') <> 'true' ELSE 1 END;

-- Backfill `blocks.due_at` and `blocks.due_millis` from each board's first `date` property.
WITH board_due_prop AS (
    SELECT
        c.id AS container_id,
        (
            SELECT json_extract(p.value, '$.id')
            FROM json_each(CASE WHEN json_valid(c.card_properties) AND json_type(c.card_properties) = 'array' THEN c.card_properties ELSE '[]' END) AS p
            WHERE json_type(p.value) = 'object'
              AND json_extract(p.value, '$.type') = 'date'
              AND json_type(p.value, '$.id') = 'text'
              AND json_extract(p.value, '$.id') <> ''
            ORDER BY p.key
            LIMIT 1
        ) AS prop_id
    FROM containers AS c
    WHERE c.kind = 'board'
),
card_due_raw AS (
    SELECT
        b.id AS block_id,
        CASE
            WHEN json_type(kv.value, '$.to') IS NOT NULL THEN json_type(kv.value, '$.to')
            ELSE json_type(kv.value, '$.from')
        END AS target_type,
        CASE
            WHEN json_type(kv.value, '$.to') IS NOT NULL THEN json_extract(kv.value, '$.to')
            ELSE json_extract(kv.value, '$.from')
        END AS target_val
    FROM blocks AS b
    JOIN board_due_prop AS dp ON dp.container_id = b.container_id AND dp.prop_id IS NOT NULL
    JOIN json_each(CASE WHEN json_valid(b.fields) AND json_type(b.fields, '$.properties') = 'object' THEN json_extract(b.fields, '$.properties') ELSE '{}' END) AS kv
        ON kv.key = dp.prop_id AND kv.type = 'object'
    WHERE b.type = 'card'
      AND CASE WHEN json_valid(b.fields) THEN COALESCE(json_type(b.fields, '$.isTemplate'), '') <> 'true' ELSE 1 END
)
UPDATE blocks
SET due_at = (
        SELECT CASE
            WHEN r.target_type = 'text' AND r.target_val GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                THEN r.target_val
            WHEN r.target_type = 'integer'
                THEN strftime(
                    '%Y-%m-%d',
                    CASE
                        WHEN r.target_val >= 0 OR (r.target_val % 1000) = 0 THEN r.target_val / 1000
                        ELSE (r.target_val / 1000) - 1
                    END,
                    'unixepoch'
                )
        END
        FROM card_due_raw AS r
        WHERE r.block_id = blocks.id
    ),
    due_millis = (
        SELECT CASE WHEN r.target_type = 'integer' THEN r.target_val END
        FROM card_due_raw AS r
        WHERE r.block_id = blocks.id
    )
WHERE id IN (
    SELECT block_id FROM card_due_raw
    WHERE (target_type = 'text' AND target_val GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]')
       OR target_type = 'integer'
);

-- Widen `audit_log` actions (`container_copied_out`, `copy_out_disabled`, `copy_out_enabled`).
CREATE TABLE audit_log_new (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_id TEXT,
    actor_kind TEXT NOT NULL DEFAULT 'person'
        CHECK (actor_kind IN ('person', 'server_operator', 'server')),
    action TEXT NOT NULL CHECK (action IN (
        'self_added_to_container', 'added_to_container',
        'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
        'org_admin_granted', 'org_admin_removed', 'org_member_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled',
        'container_copied_out', 'copy_out_disabled', 'copy_out_enabled',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator')),
    target_id TEXT NOT NULL,
    subject_id TEXT,
    created_at BIGINT NOT NULL,
    detail TEXT,
    identity TEXT,
    CHECK ((actor_kind = 'person' AND actor_id IS NOT NULL)
        OR (actor_kind IN ('server_operator', 'server') AND actor_id IS NULL)),
    CHECK (workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'org_member_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled',
        'container_copied_out', 'copy_out_disabled', 'copy_out_enabled',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator'))
);
INSERT INTO audit_log_new (id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, created_at, detail, identity)
SELECT id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, created_at, detail, identity
FROM audit_log;
DROP TABLE audit_log;
ALTER TABLE audit_log_new RENAME TO audit_log;
CREATE INDEX audit_log_workspace_id_idx ON audit_log (workspace_id, created_at);
CREATE INDEX audit_log_org_idx ON audit_log (org_id, created_at) WHERE workspace_id IS NULL;
CREATE INDEX audit_log_actor_identity_idx
    ON audit_log (actor_id) WHERE identity IS NOT NULL;
CREATE INDEX audit_log_subject_identity_idx
    ON audit_log (subject_id) WHERE identity IS NOT NULL;

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
CREATE TRIGGER audit_log_kept BEFORE DELETE ON audit_log
WHEN EXISTS (SELECT 1 FROM organizations WHERE id = OLD.org_id)
    AND (OLD.workspace_id IS NULL
         OR EXISTS (SELECT 1 FROM workspaces WHERE id = OLD.workspace_id))
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;

-- Widen `notification_outbox.kind` and `notifications.kind` (`due_soon`, `added_to_container`).
CREATE TABLE notification_outbox_new (
    org_id TEXT NOT NULL,
    recipient_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'mention', 'assignment', 'comment', 'review_due', 'meeting_upcoming',
        'due_soon', 'added_to_container')),
    source_id TEXT NOT NULL,
    block_id TEXT NOT NULL,
    container_id TEXT NOT NULL,
    actor_id TEXT,
    seq BIGINT NOT NULL,
    due_at BIGINT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, recipient_id, kind, source_id)
);
INSERT INTO notification_outbox_new (org_id, recipient_id, kind, source_id, block_id,
    container_id, actor_id, seq, due_at, attempts)
SELECT org_id, recipient_id, kind, source_id, block_id,
    container_id, actor_id, seq, due_at, attempts
FROM notification_outbox;
DROP TABLE notification_outbox;
ALTER TABLE notification_outbox_new RENAME TO notification_outbox;
CREATE INDEX notification_outbox_due_at_idx ON notification_outbox (due_at);
CREATE INDEX notification_outbox_block_idx ON notification_outbox (block_id);
CREATE INDEX notification_outbox_container_idx ON notification_outbox (container_id);

DROP INDEX notifications_same_idx;
ALTER TABLE notifications ADD COLUMN kind_wider TEXT NOT NULL DEFAULT 'mention'
    CHECK (kind_wider IN (
        'mention', 'assignment', 'comment', 'review_due', 'meeting_upcoming',
        'due_soon', 'added_to_container'));
UPDATE notifications SET kind_wider = kind;
ALTER TABLE notifications DROP COLUMN kind;
ALTER TABLE notifications RENAME COLUMN kind_wider TO kind;
CREATE INDEX notifications_same_idx
    ON notifications (recipient_id, block_id, kind, created_at);
