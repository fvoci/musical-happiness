-- Unified `created_at` and `updated_at` SQL columns across every table, index,
-- constraint, and trigger function, plus `createdAt` / `updatedAt` (and `created_at` in
-- `deleted_containers.files`) across `deleted_containers` and `deleted_blocks` JSON
-- snapshots.
--
-- `deleted_containers` and `deleted_blocks` are behind the organization wall, so the
-- system scope is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

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

DO $$
DECLARE
    found RECORD;
    target_name TEXT;
BEGIN
    FOR found IN
        SELECT cl.relname AS rel_name, c.conname AS name
        FROM pg_constraint AS c
        JOIN pg_class AS cl ON cl.oid = c.conrelid
        WHERE c.connamespace = current_schema()::regnamespace
          AND (c.conname LIKE '%create_at%' OR c.conname LIKE '%update_at%')
    LOOP
        target_name := replace(replace(found.name, 'create_at', 'created_at'), 'update_at', 'updated_at');
        IF target_name <> found.name THEN
            EXECUTE format(
                'ALTER TABLE %I RENAME CONSTRAINT %I TO %I',
                found.rel_name,
                found.name,
                target_name
            );
        END IF;
    END LOOP;
END $$;

CREATE OR REPLACE FUNCTION audit_log_is_kept() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND (
           NOT EXISTS (SELECT 1 FROM organizations WHERE id = OLD.org_id)
           OR (OLD.workspace_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM workspaces WHERE id = OLD.workspace_id))) THEN
        RETURN OLD;
    END IF;
    IF TG_OP = 'UPDATE'
       AND NEW.identity IS NULL
       AND (NEW.id, NEW.org_id, NEW.workspace_id, NEW.actor_id, NEW.actor_kind, NEW.action,
            NEW.target_id, NEW.subject_id, NEW.created_at, NEW.detail)
           IS NOT DISTINCT FROM
           (OLD.id, OLD.org_id, OLD.workspace_id, OLD.actor_id, OLD.actor_kind, OLD.action,
            OLD.target_id, OLD.subject_id, OLD.created_at, OLD.detail)
       AND EXISTS (SELECT 1 FROM users
                   WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'a record of the audit log is not changed or removed'
        USING ERRCODE = 'restrict_violation';
END $$;

DO $$
DECLARE
    r RECORD;
    doc JSONB;
    rebuilt JSONB;
    elem RECORD;
    item JSONB;
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
        WHERE container::text ~ '"(createAt|updateAt)"\s*:'
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
                    RAISE WARNING '0075: skipped unreadable deleted_containers.container row %', r.id;
                    CONTINUE;
                END;
            WHEN invalid_text_representation THEN
                RAISE WARNING '0075: skipped unreadable deleted_containers.container row %', r.id;
                CONTINUE;
        END;

        IF jsonb_typeof(doc) <> 'object' THEN
            CONTINUE;
        END IF;

        changed := false;
        IF doc ? 'createAt' THEN
            IF NOT (doc ? 'createdAt') THEN
                doc := doc || jsonb_build_object('createdAt', doc -> 'createAt');
            END IF;
            doc := doc - 'createAt';
            changed := true;
        END IF;

        IF doc ? 'updateAt' THEN
            IF NOT (doc ? 'updatedAt') THEN
                doc := doc || jsonb_build_object('updatedAt', doc -> 'updateAt');
            END IF;
            doc := doc - 'updateAt';
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
        SELECT id, org_id, files::text AS raw
        FROM deleted_containers
        WHERE files::text ~ '"create_at"\s*:'
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
                    RAISE WARNING '0075: skipped unreadable deleted_containers.files row %', r.id;
                    CONTINUE;
                END;
            WHEN invalid_text_representation THEN
                RAISE WARNING '0075: skipped unreadable deleted_containers.files row %', r.id;
                CONTINUE;
        END;

        IF jsonb_typeof(doc) <> 'array' OR jsonb_array_length(doc) = 0 THEN
            CONTINUE;
        END IF;

        rebuilt := '[]'::jsonb;
        changed := false;
        FOR elem IN
            SELECT val, ord
            FROM jsonb_array_elements(doc) WITH ORDINALITY AS t(val, ord)
            ORDER BY ord
        LOOP
            item := elem.val;
            IF jsonb_typeof(item) = 'object' AND (item ? 'create_at') THEN
                IF NOT (item ? 'created_at') THEN
                    item := item || jsonb_build_object('created_at', item -> 'create_at');
                END IF;
                item := item - 'create_at';
                changed := true;
            END IF;
            rebuilt := rebuilt || jsonb_build_array(item);
        END LOOP;

        IF NOT changed THEN
            CONTINUE;
        END IF;

        IF has_nul THEN
            rewritten := replace(rebuilt::text, nul_sentinel, '\u0000')::json;
        ELSE
            rewritten := rebuilt::json;
        END IF;

        UPDATE deleted_containers
        SET files = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;

    FOR r IN
        SELECT id, org_id, block::text AS raw
        FROM deleted_blocks
        WHERE block::text ~ '"(createAt|updateAt)"\s*:'
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
                    RAISE WARNING '0075: skipped unreadable deleted_blocks.block row %', r.id;
                    CONTINUE;
                END;
            WHEN invalid_text_representation THEN
                RAISE WARNING '0075: skipped unreadable deleted_blocks.block row %', r.id;
                CONTINUE;
        END;

        IF jsonb_typeof(doc) <> 'object' THEN
            CONTINUE;
        END IF;

        changed := false;
        IF doc ? 'createAt' THEN
            IF NOT (doc ? 'createdAt') THEN
                doc := doc || jsonb_build_object('createdAt', doc -> 'createAt');
            END IF;
            doc := doc - 'createAt';
            changed := true;
        END IF;

        IF doc ? 'updateAt' THEN
            IF NOT (doc ? 'updatedAt') THEN
                doc := doc || jsonb_build_object('updatedAt', doc -> 'updateAt');
            END IF;
            doc := doc - 'updateAt';
            changed := true;
        END IF;

        IF jsonb_typeof(doc -> 'watchedBy') = 'array'
           AND jsonb_array_length(doc -> 'watchedBy') > 0
        THEN
            rebuilt := '[]'::jsonb;
            FOR elem IN
                SELECT val, ord
                FROM jsonb_array_elements(doc -> 'watchedBy') WITH ORDINALITY AS t(val, ord)
                ORDER BY ord
            LOOP
                item := elem.val;
                IF jsonb_typeof(item) = 'object' AND (item ? 'createAt') THEN
                    IF NOT (item ? 'createdAt') THEN
                        item := item || jsonb_build_object('createdAt', item -> 'createAt');
                    END IF;
                    item := item - 'createAt';
                    changed := true;
                END IF;
                rebuilt := rebuilt || jsonb_build_array(item);
            END LOOP;
            doc := jsonb_set(doc, '{watchedBy}', rebuilt);
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
