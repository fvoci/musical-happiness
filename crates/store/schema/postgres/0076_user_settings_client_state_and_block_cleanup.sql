-- Dedicated `user_settings` and `user_client_state` global tables (retiring `preferences`),
-- removal of `blocks."schema"` and `deleted_blocks.block` `schema`, and `organizations.kind`
-- `'shared'` (replacing `'team'`).
--
-- `deleted_blocks` and `organizations` are behind the organization wall, so the system scope
-- is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE user_settings (
    user_id UUID NOT NULL PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    language TEXT NOT NULL DEFAULT 'auto',
    theme TEXT NOT NULL DEFAULT 'auto' CHECK (theme IN ('auto', 'light', 'dark')),
    density TEXT NOT NULL DEFAULT 'auto' CHECK (density IN ('auto', 'compact', 'comfortable')),
    card_open TEXT NOT NULL DEFAULT 'center' CHECK (card_open IN ('center', 'side', 'full')),
    haptics BOOLEAN NOT NULL DEFAULT TRUE,
    week_start TEXT NOT NULL DEFAULT 'auto'
        CHECK (week_start IN ('auto', 'sunday', 'monday', 'saturday')),
    hour_cycle TEXT NOT NULL DEFAULT 'auto' CHECK (hour_cycle IN ('auto', 'h12', 'h23')),
    time_zone TEXT NOT NULL DEFAULT 'auto',
    notify_mentions BOOLEAN NOT NULL DEFAULT TRUE,
    notify_assignments BOOLEAN NOT NULL DEFAULT TRUE,
    notify_comments BOOLEAN NOT NULL DEFAULT TRUE,
    notify_reviews BOOLEAN NOT NULL DEFAULT TRUE,
    notify_meetings BOOLEAN NOT NULL DEFAULT TRUE,
    notify_due BOOLEAN NOT NULL DEFAULT TRUE
);

INSERT INTO user_settings (
    user_id,
    language,
    theme,
    density,
    card_open,
    haptics,
    week_start,
    hour_cycle,
    time_zone,
    notify_mentions,
    notify_assignments,
    notify_comments
)
SELECT
    p.user_id,
    COALESCE(MAX(CASE WHEN p.name = 'language' AND p.value <> '' THEN p.value END), 'auto'),
    COALESCE(
        MAX(CASE WHEN p.name = 'theme' AND p.value IN ('auto', 'light', 'dark') THEN p.value END),
        'auto'
    ),
    COALESCE(
        MAX(
            CASE
                WHEN p.name = 'density' AND p.value IN ('auto', 'compact', 'comfortable')
                    THEN p.value
            END
        ),
        'auto'
    ),
    COALESCE(
        MAX(
            CASE
                WHEN p.name = 'cardOpen' AND p.value IN ('center', 'side', 'full') THEN p.value
            END
        ),
        'center'
    ),
    CASE COALESCE(
        MAX(CASE WHEN p.name = 'haptics' AND p.value IN ('true', 'false') THEN p.value END),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END,
    COALESCE(
        MAX(
            CASE
                WHEN p.name = 'weekStart'
                     AND p.value IN ('auto', 'sunday', 'monday', 'saturday')
                    THEN p.value
            END
        ),
        'auto'
    ),
    COALESCE(
        MAX(CASE WHEN p.name = 'hourCycle' AND p.value IN ('auto', 'h12', 'h23') THEN p.value END),
        'auto'
    ),
    COALESCE(MAX(CASE WHEN p.name = 'timeZone' AND p.value <> '' THEN p.value END), 'auto'),
    CASE COALESCE(
        MAX(CASE WHEN p.name = 'notifyMentions' AND p.value IN ('true', 'false') THEN p.value END),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END,
    CASE COALESCE(
        MAX(
            CASE
                WHEN p.name = 'notifyAssignments' AND p.value IN ('true', 'false') THEN p.value
            END
        ),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END,
    CASE COALESCE(
        MAX(CASE WHEN p.name = 'notifyComments' AND p.value IN ('true', 'false') THEN p.value END),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END
FROM preferences AS p
WHERE p.category = 'settings'
GROUP BY p.user_id;

CREATE TABLE user_client_state (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
);

INSERT INTO user_client_state (user_id, key, value)
SELECT user_id, name, value
FROM preferences
WHERE category = 'client';

DROP TABLE preferences;

ALTER TABLE blocks DROP COLUMN "schema";

ALTER TABLE blocks ADD CONSTRAINT blocks_type_check
    CHECK (type IN ('card', 'view', 'comment', 'attachment', 'page', 'entry', 'unit', 'chapter'));

UPDATE subscriptions
SET block_type = (SELECT b.type FROM blocks AS b WHERE b.id = subscriptions.block_id)
WHERE block_type NOT IN ('card', 'view', 'comment', 'attachment', 'page', 'entry', 'unit', 'chapter');

DO $$
DECLARE
    r RECORD;
    doc JSONB;
    has_nul BOOLEAN;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, block::text AS raw
        FROM deleted_blocks
        WHERE block::text ~ '"schema"\s*:'
    LOOP
        has_nul := false;
        BEGIN
            doc := r.raw::jsonb;
        EXCEPTION
            WHEN untranslatable_character THEN
                has_nul := true;
                bs_sentinel := '__FVOCI_BS_' || md5(r.raw) || '__';
                nul_sentinel := '__FVOCI_NUL_' || md5(r.raw) || '__';
                safe := replace(r.raw, '\\', bs_sentinel);
                safe := replace(safe, '\u0000', nul_sentinel);
                safe := replace(safe, bs_sentinel, '\\');
                BEGIN
                    doc := safe::jsonb;
                EXCEPTION WHEN invalid_text_representation OR untranslatable_character THEN
                    RAISE WARNING '0076: skipped unreadable deleted_blocks.block row %', r.id;
                    CONTINUE;
                END;
            WHEN invalid_text_representation THEN
                RAISE WARNING '0076: skipped unreadable deleted_blocks.block row %', r.id;
                CONTINUE;
        END;

        IF jsonb_typeof(doc) <> 'object' OR NOT (doc ? 'schema') THEN
            CONTINUE;
        END IF;

        doc := doc - 'schema';

        IF has_nul THEN
            rewritten := replace(doc::text, nul_sentinel, '\u0000')::json;
        ELSE
            rewritten := doc::json;
        END IF;

        UPDATE deleted_blocks
        SET block = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;
END $$;

DROP TRIGGER organizations_keep_their_kind ON organizations;
DROP INDEX organizations_of_people_idx;

DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conname AS name
        FROM pg_constraint AS c
        JOIN pg_class AS cl ON cl.oid = c.conrelid
        WHERE cl.relnamespace = current_schema()::regnamespace
          AND cl.relname = 'organizations'
          AND c.contype = 'c'
          AND pg_get_constraintdef(c.oid) LIKE '%team%'
    LOOP
        EXECUTE format('ALTER TABLE organizations DROP CONSTRAINT %I', found.name);
    END LOOP;
END $$;

ALTER TABLE organizations ALTER COLUMN kind SET DEFAULT 'shared';
UPDATE organizations SET kind = 'shared' WHERE kind = 'team';

ALTER TABLE organizations ADD CONSTRAINT organizations_kind_check
    CHECK (kind IN ('shared', 'personal'));
ALTER TABLE organizations ADD CONSTRAINT organizations_system_is_no_ones
    CHECK (NOT is_system OR kind = 'shared');

CREATE INDEX organizations_of_people_idx ON organizations (created_at, id)
    WHERE kind = 'shared';

CREATE TRIGGER organizations_keep_their_kind BEFORE UPDATE OF kind, owner_id ON organizations
    FOR EACH ROW
    WHEN (NEW.kind IS DISTINCT FROM OLD.kind OR NEW.owner_id IS DISTINCT FROM OLD.owner_id)
    EXECUTE FUNCTION organizations_keep_their_kind();
