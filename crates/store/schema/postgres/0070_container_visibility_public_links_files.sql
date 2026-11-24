-- Container visibility (`open` or `private`, replacing `containers.type` `'O'`/`'P'`),
-- `public_links` (replacing `sharing`), and `files` (replacing `file_info`).
--
-- `containers`, `deleted_containers`, `public_links` and `files` are behind the
-- organization wall, so the system scope is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE containers DROP CONSTRAINT containers_type_check;
ALTER TABLE containers RENAME COLUMN type TO visibility;
UPDATE containers
SET visibility = CASE visibility
    WHEN 'O' THEN 'open'
    WHEN 'P' THEN 'private'
END;
ALTER TABLE containers ADD CONSTRAINT containers_visibility_check
    CHECK (visibility IN ('open', 'private'));

DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace
            AND c.conrelid = 'containers'::regclass
            AND c.conname LIKE 'containers_%type%'
    LOOP
        EXECUTE format(
            'ALTER TABLE containers RENAME CONSTRAINT %I TO %I',
            found.name,
            replace(found.name, 'type', 'visibility')
        );
    END LOOP;
END $$;

ALTER TABLE sharing RENAME TO public_links;
ALTER TRIGGER sharing_of_no_diary ON public_links RENAME TO public_links_of_no_diary;

DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace
            AND c.conrelid = 'public_links'::regclass
            AND c.conname LIKE 'sharing%'
    LOOP
        EXECUTE format(
            'ALTER TABLE public_links RENAME CONSTRAINT %I TO %I',
            found.name,
            replace(found.name, 'sharing', 'public_links')
        );
    END LOOP;

    FOR found IN
        SELECT c.relname AS name
        FROM pg_class AS c
        WHERE c.relnamespace = current_schema()::regnamespace
            AND c.relkind = 'i'
            AND c.relname LIKE 'sharing%'
    LOOP
        EXECUTE format(
            'ALTER INDEX %I RENAME TO %I',
            found.name,
            replace(found.name, 'sharing', 'public_links')
        );
    END LOOP;
END $$;

ALTER TABLE file_info RENAME TO files;

DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace
            AND c.conrelid = 'files'::regclass
            AND c.conname LIKE 'file_info%'
    LOOP
        EXECUTE format(
            'ALTER TABLE files RENAME CONSTRAINT %I TO %I',
            found.name,
            replace(found.name, 'file_info', 'files')
        );
    END LOOP;

    FOR found IN
        SELECT c.relname AS name
        FROM pg_class AS c
        WHERE c.relnamespace = current_schema()::regnamespace
            AND c.relkind = 'i'
            AND c.relname LIKE 'file_info%'
    LOOP
        EXECUTE format(
            'ALTER INDEX %I RENAME TO %I',
            found.name,
            replace(found.name, 'file_info', 'files')
        );
    END LOOP;
END $$;

DO $$
DECLARE
    r RECORD;
    doc JSONB;
    val JSONB;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, container::text AS raw
        FROM deleted_containers
        WHERE container::text ~ '"type"\s*:'
    LOOP
        BEGIN
            doc := r.raw::jsonb;
            IF NOT (doc ? 'type') THEN
                CONTINUE;
            END IF;
            val := CASE doc ->> 'type'
                WHEN 'O' THEN '"open"'::jsonb
                WHEN 'P' THEN '"private"'::jsonb
                ELSE doc -> 'type'
            END;
            rewritten := ((doc - 'type') || jsonb_build_object('visibility', val))::json;
        EXCEPTION WHEN untranslatable_character THEN
            bs_sentinel := '__FVOCI_BS_' || md5(r.raw) || '__';
            nul_sentinel := '__FVOCI_NUL_' || md5(r.raw) || '__';
            safe := replace(r.raw, '\\', bs_sentinel);
            safe := replace(safe, '\u0000', nul_sentinel);
            safe := replace(safe, bs_sentinel, '\\');
            doc := safe::jsonb;
            IF NOT (doc ? 'type') THEN
                CONTINUE;
            END IF;
            val := CASE doc ->> 'type'
                WHEN 'O' THEN '"open"'::jsonb
                WHEN 'P' THEN '"private"'::jsonb
                ELSE doc -> 'type'
            END;
            doc := (doc - 'type') || jsonb_build_object('visibility', val);
            rewritten := replace(doc::text, nul_sentinel, '\u0000')::json;
        END;

        UPDATE deleted_containers
        SET container = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;
END $$;
