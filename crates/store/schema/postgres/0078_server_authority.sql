-- Server-authority schema additions: `workspaces.welcomed_at`,
-- `organizations.allow_copy_out`, board completion and due-date metadata
-- (`containers.due_date_property_id`, `containers.done_when`), derived card due/done
-- columns (`blocks.due_at`, `blocks.due_millis`, `blocks.is_done`), `card_assignees`,
-- `card_due_notices`, and widened audit/notification kinds.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE workspaces ADD COLUMN welcomed_at BIGINT
    CHECK (welcomed_at IS NULL OR welcomed_at > 0);

UPDATE workspaces
SET welcomed_at = CASE WHEN created_at > 0 THEN created_at ELSE 1 END
WHERE is_system = FALSE
  AND (
      EXISTS (
          SELECT 1 FROM containers AS c
          WHERE c.workspace_id = workspaces.id AND c.org_id = workspaces.org_id
      )
      OR EXISTS (
          SELECT 1 FROM deleted_containers AS dc
          WHERE dc.workspace_id = workspaces.id AND dc.org_id = workspaces.org_id
      )
      OR EXISTS (
          SELECT 1 FROM workspace_members AS wm
          JOIN users AS u ON u.id = wm.user_id
          WHERE wm.workspace_id = workspaces.id
            AND wm.org_id = workspaces.org_id
            AND u.welcomed_at IS NOT NULL
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
ALTER TABLE containers ADD COLUMN done_when JSONB;

ALTER TABLE blocks ADD COLUMN due_at TEXT;
ALTER TABLE blocks ADD COLUMN due_millis BIGINT;
ALTER TABLE blocks ADD COLUMN is_done BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX blocks_due_at_idx
    ON blocks (container_id, due_at)
    WHERE due_at IS NOT NULL AND is_done = FALSE;

CREATE TABLE card_assignees (
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    container_id UUID NOT NULL,
    card_id UUID NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (org_id, card_id, user_id),
    FOREIGN KEY (container_id, org_id) REFERENCES containers (id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (container_id, card_id) REFERENCES blocks (container_id, id) ON DELETE CASCADE
);

CREATE INDEX card_assignees_user_container_idx
    ON card_assignees (org_id, user_id, container_id);
CREATE INDEX card_assignees_container_idx
    ON card_assignees (container_id);
CREATE INDEX card_assignees_user_idx
    ON card_assignees (user_id);

ALTER TABLE card_assignees ENABLE ROW LEVEL SECURITY;
ALTER TABLE card_assignees FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON card_assignees USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

CREATE TABLE card_due_notices (
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    card_id UUID NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    due_key TEXT NOT NULL,
    notified_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, card_id, user_id, due_key)
);

CREATE INDEX card_due_notices_user_idx
    ON card_due_notices (user_id);
CREATE INDEX card_due_notices_card_idx
    ON card_due_notices (card_id);

ALTER TABLE card_due_notices ENABLE ROW LEVEL SECURITY;
ALTER TABLE card_due_notices FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON card_due_notices USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Backfill `card_assignees` for existing live non-template cards on boards.
INSERT INTO card_assignees (org_id, container_id, card_id, user_id)
SELECT b.org_id, c.id, b.id, u.id
FROM blocks AS b
JOIN containers AS c ON c.id = b.container_id AND c.org_id = b.org_id
    AND c.kind = 'board' AND c.is_template = FALSE
CROSS JOIN LATERAL jsonb_array_elements(
    CASE WHEN jsonb_typeof(c.card_properties) = 'array' THEN c.card_properties ELSE '[]'::jsonb END
) AS prop(val)
JOIN users AS u ON u.id::text = (b.fields -> 'properties' ->> (prop.val ->> 'id'))
WHERE b.type = 'card'
  AND b.fields::text !~ '\\u0000'
  AND json_typeof(b.fields) = 'object'
  AND COALESCE(b.fields ->> 'isTemplate', 'false') <> 'true'
  AND json_typeof(b.fields -> 'properties') = 'object'
  AND jsonb_typeof(prop.val) = 'object'
  AND prop.val ->> 'type' = 'person'
  AND jsonb_typeof(prop.val -> 'id') = 'string'
  AND json_typeof(b.fields -> 'properties' -> (prop.val ->> 'id')) = 'string'
ON CONFLICT DO NOTHING;

INSERT INTO card_assignees (org_id, container_id, card_id, user_id)
SELECT b.org_id, c.id, b.id, u.id
FROM blocks AS b
JOIN containers AS c ON c.id = b.container_id AND c.org_id = b.org_id
    AND c.kind = 'board' AND c.is_template = FALSE
CROSS JOIN LATERAL jsonb_array_elements(
    CASE WHEN jsonb_typeof(c.card_properties) = 'array' THEN c.card_properties ELSE '[]'::jsonb END
) AS prop(val)
-- The argument holds no value of another kind: what filters it below is not sure to be
-- asked first.
CROSS JOIN LATERAL json_array_elements(
    CASE WHEN json_typeof(b.fields -> 'properties' -> (prop.val ->> 'id')) = 'array'
        THEN b.fields -> 'properties' -> (prop.val ->> 'id') ELSE '[]'::json END
) AS item(val)
JOIN users AS u ON u.id::text = (item.val #>> '{}')
WHERE b.type = 'card'
  AND b.fields::text !~ '\\u0000'
  AND json_typeof(b.fields) = 'object'
  AND COALESCE(b.fields ->> 'isTemplate', 'false') <> 'true'
  AND json_typeof(b.fields -> 'properties') = 'object'
  AND jsonb_typeof(prop.val) = 'object'
  AND prop.val ->> 'type' = 'multiPerson'
  AND jsonb_typeof(prop.val -> 'id') = 'string'
  AND json_typeof(b.fields -> 'properties' -> (prop.val ->> 'id')) = 'array'
  AND json_typeof(item.val) = 'string'
ON CONFLICT DO NOTHING;

-- Backfill `blocks.due_at` and `blocks.due_millis` from each board's first `date` property.
WITH board_due_prop AS (
    SELECT
        c.id AS container_id,
        c.org_id AS org_id,
        (
            SELECT p.val ->> 'id'
            FROM jsonb_array_elements(
                CASE WHEN jsonb_typeof(c.card_properties) = 'array' THEN c.card_properties ELSE '[]'::jsonb END
            ) WITH ORDINALITY AS p(val, ord)
            WHERE jsonb_typeof(p.val) = 'object'
              AND p.val ->> 'type' = 'date'
              AND jsonb_typeof(p.val -> 'id') = 'string'
              AND (p.val ->> 'id') <> ''
            ORDER BY p.ord
            LIMIT 1
        ) AS prop_id
    FROM containers AS c
    WHERE c.kind = 'board' AND c.is_template = FALSE
),
card_due_raw AS (
    SELECT
        b.id AS block_id,
        b.org_id AS org_id,
        COALESCE(
            b.fields -> 'properties' -> dp.prop_id -> 'to',
            b.fields -> 'properties' -> dp.prop_id -> 'from'
        ) AS target
    FROM blocks AS b
    JOIN board_due_prop AS dp
        ON dp.container_id = b.container_id AND dp.org_id = b.org_id AND dp.prop_id IS NOT NULL
    WHERE b.type = 'card'
      AND b.fields::text !~ '\\u0000'
      AND json_typeof(b.fields) = 'object'
      AND COALESCE(b.fields ->> 'isTemplate', 'false') <> 'true'
      AND json_typeof(b.fields -> 'properties') = 'object'
      AND json_typeof(b.fields -> 'properties' -> dp.prop_id) = 'object'
),
-- A day as it is written, or a time in milliseconds of at most 15 digits, which a bigint
-- holds: CASE asks the pattern before the cast. Of a time, only one in the years 1 to
-- 9999 is taken, as the server takes it.
card_due AS (
    SELECT
        r.block_id,
        r.org_id,
        CASE WHEN json_typeof(r.target) = 'string' AND (r.target #>> '{}') ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
            THEN r.target #>> '{}' END AS day,
        CASE WHEN json_typeof(r.target) = 'number' AND (r.target::text) ~ '^-?[0-9]{1,15}$'
            THEN (r.target::text)::bigint END AS millis
    FROM card_due_raw AS r
)
UPDATE blocks AS b
SET due_at = COALESCE(
        d.day,
        to_char(to_timestamp(d.millis / 1000.0) AT TIME ZONE 'UTC', 'YYYY-MM-DD')
    ),
    due_millis = d.millis
FROM card_due AS d
WHERE b.id = d.block_id
  AND b.org_id = d.org_id
  AND (d.day IS NOT NULL OR d.millis BETWEEN -62135596800000 AND 253402300799999);

-- Widen `audit_log` actions (`container_copied_out`, `copy_out_disabled`, `copy_out_enabled`).
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
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
    'oidc_former_provider_lapsed', 'oidc_identity_bound', 'oidc_identity_carried_over',
    'email_verified_by_operator'));

ALTER TABLE audit_log DROP CONSTRAINT audit_log_place_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
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
        'oidc_identity_carried_over', 'email_verified_by_operator'));

-- Widen `notification_outbox.kind` and `notifications.kind` (`due_soon`, `added_to_container`).
ALTER TABLE notification_outbox DROP CONSTRAINT notification_outbox_kind_check;
ALTER TABLE notification_outbox ADD CONSTRAINT notification_outbox_kind_check
    CHECK (kind IN (
        'mention', 'assignment', 'comment', 'review_due', 'meeting_upcoming',
        'due_soon', 'added_to_container'));

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN (
        'mention', 'assignment', 'comment', 'review_due', 'meeting_upcoming',
        'due_soon', 'added_to_container'));
