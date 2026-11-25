-- Single `role` enum (`admin`, `editor`, `commenter`, `viewer`) on `container_members`,
-- `deleted_containers.members` snapshots, and `workspace_members.prior_container_roles`.
-- Legacy rows and snapshot entries where every `scheme_*` flag was false held no role
-- and are pruned rather than elevated to `viewer`.
--
-- `container_members`, `deleted_containers`, and `workspace_members` are behind the
-- organization wall, so the system scope is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

DELETE FROM container_members
WHERE NOT (
    COALESCE(scheme_admin, false)
    OR COALESCE(scheme_editor, false)
    OR COALESCE(scheme_commenter, false)
    OR COALESCE(scheme_viewer, false)
);

ALTER TABLE container_members ADD COLUMN role TEXT;

UPDATE container_members
SET role = CASE
    WHEN scheme_admin THEN 'admin'
    WHEN scheme_editor THEN 'editor'
    WHEN scheme_commenter THEN 'commenter'
    ELSE 'viewer'
END;

ALTER TABLE container_members ALTER COLUMN role SET NOT NULL;
ALTER TABLE container_members ADD CONSTRAINT container_members_role_check
    CHECK (role IN ('admin', 'editor', 'commenter', 'viewer'));

ALTER TABLE container_members
    DROP COLUMN scheme_admin,
    DROP COLUMN scheme_editor,
    DROP COLUMN scheme_commenter,
    DROP COLUMN scheme_viewer;

DO $$
DECLARE
    r RECORD;
    doc JSONB;
    rebuilt JSONB;
    elem RECORD;
    item JSONB;
    role_val TEXT;
    changed BOOLEAN;
    has_nul BOOLEAN;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, members::text AS raw
        FROM deleted_containers
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
                    CONTINUE;
                END;
            WHEN invalid_text_representation THEN
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
            IF jsonb_typeof(item) = 'object'
               AND (
                   item ? 'schemeAdmin'
                   OR item ? 'schemeEditor'
                   OR item ? 'schemeCommenter'
                   OR item ? 'schemeViewer'
                   OR NOT (
                       jsonb_typeof(item -> 'role') = 'string'
                       AND (item ->> 'role') IN ('admin', 'editor', 'commenter', 'viewer')
                   )
               )
            THEN
                IF jsonb_typeof(item -> 'role') = 'string'
                   AND (item ->> 'role') IN ('admin', 'editor', 'commenter', 'viewer')
                THEN
                    role_val := item ->> 'role';
                ELSIF item -> 'schemeAdmin' = 'true'::jsonb THEN
                    role_val := 'admin';
                ELSIF item -> 'schemeEditor' = 'true'::jsonb THEN
                    role_val := 'editor';
                ELSIF item -> 'schemeCommenter' = 'true'::jsonb THEN
                    role_val := 'commenter';
                ELSIF item -> 'schemeViewer' = 'true'::jsonb THEN
                    role_val := 'viewer';
                ELSE
                    changed := true;
                    CONTINUE;
                END IF;
                item := (item - 'schemeAdmin' - 'schemeEditor' - 'schemeCommenter' - 'schemeViewer')
                    || jsonb_build_object('role', role_val);
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
        SET members = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;

    FOR r IN
        SELECT workspace_id, user_id, org_id, prior_container_roles AS raw
        FROM workspace_members
        WHERE prior_container_roles IS NOT NULL
    LOOP
        BEGIN
            doc := r.raw::jsonb;
        EXCEPTION WHEN invalid_text_representation OR untranslatable_character THEN
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
            IF jsonb_typeof(item) = 'array'
               AND jsonb_array_length(item) >= 2
               AND jsonb_typeof(item -> 1) = 'array'
            THEN
                IF (item -> 1 -> 0) = 'true'::jsonb THEN
                    role_val := 'admin';
                ELSIF (item -> 1 -> 1) = 'true'::jsonb THEN
                    role_val := 'editor';
                ELSIF (item -> 1 -> 2) = 'true'::jsonb THEN
                    role_val := 'commenter';
                ELSIF (item -> 1 -> 3) = 'true'::jsonb THEN
                    role_val := 'viewer';
                ELSE
                    changed := true;
                    CONTINUE;
                END IF;
                item := jsonb_build_array(item -> 0, to_jsonb(role_val));
                changed := true;
            END IF;
            rebuilt := rebuilt || jsonb_build_array(item);
        END LOOP;

        IF changed THEN
            UPDATE workspace_members
            SET prior_container_roles = rebuilt::text
            WHERE workspace_id = r.workspace_id
              AND user_id = r.user_id
              AND org_id = r.org_id;
        END IF;
    END LOOP;
END $$;
