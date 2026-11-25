-- Nullable `deleted_at` (`NULL` while in use, positive epoch milliseconds once deleted
-- or waiting to be erased) across `organizations`, `workspaces`, and `users`, plus
-- `deletedAt` (omitted while live) in `deleted_containers` and `deleted_blocks` JSON.
--
-- `organizations`, `workspaces`, `deleted_containers`, and `deleted_blocks` are behind
-- the organization wall, so the system scope is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE organizations RENAME COLUMN delete_at TO deleted_at;
ALTER TABLE organizations ALTER COLUMN deleted_at DROP NOT NULL;
ALTER TABLE organizations ALTER COLUMN deleted_at DROP DEFAULT;
UPDATE organizations
SET deleted_at = CASE WHEN deleted_at > 0 THEN deleted_at ELSE NULL END;
ALTER TABLE organizations ADD CONSTRAINT organizations_deleted_at_check
    CHECK (deleted_at IS NULL OR deleted_at > 0);

DROP INDEX workspaces_name_in_use_key;
DROP INDEX workspaces_deleted_idx;
ALTER TABLE workspaces RENAME COLUMN delete_at TO deleted_at;
ALTER TABLE workspaces ALTER COLUMN deleted_at DROP NOT NULL;
ALTER TABLE workspaces ALTER COLUMN deleted_at DROP DEFAULT;
UPDATE workspaces
SET deleted_at = CASE WHEN deleted_at > 0 THEN deleted_at ELSE NULL END;
ALTER TABLE workspaces ADD CONSTRAINT workspaces_deleted_at_check
    CHECK (deleted_at IS NULL OR deleted_at > 0);
CREATE UNIQUE INDEX workspaces_name_in_use_key ON workspaces (org_id, name)
    WHERE deleted_at IS NULL AND name <> '';
CREATE INDEX workspaces_deleted_idx ON workspaces (deleted_at)
    WHERE deleted_at IS NOT NULL;

DROP INDEX users_leaving_idx;

DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace
            AND c.conrelid = 'users'::regclass
            AND c.contype = 'c'
            AND pg_get_constraintdef(c.oid) LIKE '%delete_at%'
    LOOP
        EXECUTE format('ALTER TABLE users DROP CONSTRAINT %I', found.name);
    END LOOP;
END $$;

ALTER TABLE users RENAME COLUMN delete_at TO deleted_at;
ALTER TABLE users ALTER COLUMN deleted_at DROP NOT NULL;
ALTER TABLE users ALTER COLUMN deleted_at DROP DEFAULT;
UPDATE users
SET deleted_at = CASE WHEN deleted_at > 0 THEN deleted_at ELSE NULL END;
ALTER TABLE users ADD CONSTRAINT users_deleted_at_check
    CHECK (deleted_at IS NULL OR deleted_at > 0);
ALTER TABLE users ADD CONSTRAINT users_delete_after_check
    CHECK (delete_after = 0 OR (deleted_at IS NOT NULL AND deleted_at > 0));
ALTER TABLE users ADD CONSTRAINT users_erase_started_at_check
    CHECK (erase_started_at = 0 OR (deleted_at IS NOT NULL AND deleted_at > 0));
CREATE INDEX users_leaving_idx ON users (delete_after)
    WHERE deleted_at IS NOT NULL AND erased_at = 0;

DO $$
DECLARE
    key_pattern CONSTANT TEXT := '"deleted?At"\s*:';
    r RECORD;
    doc JSONB;
    changed BOOLEAN;
    has_nul BOOLEAN;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, container::text AS raw
        FROM deleted_containers
        WHERE container::text ~ key_pattern
    LOOP
        has_nul := false;
        BEGIN
            doc := r.raw::jsonb;
        EXCEPTION WHEN untranslatable_character THEN
            has_nul := true;
            bs_sentinel := '__FVOCI_BS_' || md5(r.raw) || '__';
            nul_sentinel := '__FVOCI_NUL_' || md5(r.raw) || '__';
            safe := replace(r.raw, '\\', bs_sentinel);
            safe := replace(safe, '\u0000', nul_sentinel);
            safe := replace(safe, bs_sentinel, '\\');
            doc := safe::jsonb;
        END;

        IF jsonb_typeof(doc) <> 'object' THEN
            CONTINUE;
        END IF;

        changed := false;
        IF doc ? 'deleteAt' THEN
            IF NOT (doc ? 'deletedAt')
               AND jsonb_typeof(doc -> 'deleteAt') = 'number'
               AND (doc ->> 'deleteAt')::numeric > 0
            THEN
                doc := doc || jsonb_build_object('deletedAt', doc -> 'deleteAt');
            END IF;
            doc := doc - 'deleteAt';
            changed := true;
        END IF;

        IF doc ? 'deletedAt'
           AND (
               jsonb_typeof(doc -> 'deletedAt') = 'null'
               OR (
                   jsonb_typeof(doc -> 'deletedAt') = 'number'
                   AND (doc ->> 'deletedAt')::numeric <= 0
               )
           )
        THEN
            doc := doc - 'deletedAt';
            changed := true;
        END IF;

        IF NOT changed THEN
            CONTINUE;
        END IF;

        IF has_nul THEN
            rewritten := replace(doc::text, nul_sentinel, '\u0000')::json;
        ELSE
            rewritten := doc::json;
        END IF;

        UPDATE deleted_containers
        SET container = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;

    FOR r IN
        SELECT id, org_id, block::text AS raw
        FROM deleted_blocks
        WHERE block::text ~ key_pattern
    LOOP
        has_nul := false;
        BEGIN
            doc := r.raw::jsonb;
        EXCEPTION WHEN untranslatable_character THEN
            has_nul := true;
            bs_sentinel := '__FVOCI_BS_' || md5(r.raw) || '__';
            nul_sentinel := '__FVOCI_NUL_' || md5(r.raw) || '__';
            safe := replace(r.raw, '\\', bs_sentinel);
            safe := replace(safe, '\u0000', nul_sentinel);
            safe := replace(safe, bs_sentinel, '\\');
            doc := safe::jsonb;
        END;

        IF jsonb_typeof(doc) <> 'object' THEN
            CONTINUE;
        END IF;

        changed := false;
        IF doc ? 'deleteAt' THEN
            IF NOT (doc ? 'deletedAt')
               AND jsonb_typeof(doc -> 'deleteAt') = 'number'
               AND (doc ->> 'deleteAt')::numeric > 0
            THEN
                doc := doc || jsonb_build_object('deletedAt', doc -> 'deleteAt');
            END IF;
            doc := doc - 'deleteAt';
            changed := true;
        END IF;

        IF doc ? 'deletedAt'
           AND (
               jsonb_typeof(doc -> 'deletedAt') = 'null'
               OR (
                   jsonb_typeof(doc -> 'deletedAt') = 'number'
                   AND (doc ->> 'deletedAt')::numeric <= 0
               )
           )
        THEN
            doc := doc - 'deletedAt';
            changed := true;
        END IF;

        IF NOT changed THEN
            CONTINUE;
        END IF;

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
