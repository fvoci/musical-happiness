-- Native view `fields` keys across `blocks` (`type = 'view'`) and `deleted_blocks`
-- (`block->>'type' = 'view'`):
--   visibleOptionIds      -> visibleGroups
--   hiddenOptionIds       -> hiddenGroups
--   collapsedOptionIds    -> collapsedGroups
--   kanbanCalculations    -> groupCalculations
--   visiblePropertyIds    -> shownProperties
--   groupById             -> groupBy
--   sortOptions           -> sort
--   dateDisplayPropertyId -> dateProperty
--
-- `blocks` and `deleted_blocks` are behind the organization wall, so the system scope
-- is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

DO $$
DECLARE
    pairs CONSTANT TEXT[][] := ARRAY[
        ARRAY['visibleOptionIds', 'visibleGroups'],
        ARRAY['hiddenOptionIds', 'hiddenGroups'],
        ARRAY['collapsedOptionIds', 'collapsedGroups'],
        ARRAY['kanbanCalculations', 'groupCalculations'],
        ARRAY['visiblePropertyIds', 'shownProperties'],
        ARRAY['groupById', 'groupBy'],
        ARRAY['sortOptions', 'sort'],
        ARRAY['dateDisplayPropertyId', 'dateProperty']
    ];
    key_pattern CONSTANT TEXT := '"(visibleOptionIds|hiddenOptionIds|collapsedOptionIds|kanbanCalculations|visiblePropertyIds|groupById|sortOptions|dateDisplayPropertyId)"\s*:';
    r RECORD;
    doc JSONB;
    fields_obj JSONB;
    pair TEXT[];
    changed BOOLEAN;
    has_nul BOOLEAN;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, fields::text AS raw
        FROM blocks
        WHERE type = 'view'
          AND fields::text ~ key_pattern
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
        FOREACH pair SLICE 1 IN ARRAY pairs LOOP
            IF doc ? pair[1] THEN
                IF NOT (doc ? pair[2]) THEN
                    doc := doc || jsonb_build_object(pair[2], doc -> pair[1]);
                END IF;
                doc := doc - pair[1];
                changed := true;
            END IF;
        END LOOP;

        IF NOT changed THEN
            CONTINUE;
        END IF;

        IF has_nul THEN
            rewritten := replace(doc::text, nul_sentinel, '\u0000')::json;
        ELSE
            rewritten := doc::json;
        END IF;

        UPDATE blocks
        SET fields = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;

    FOR r IN
        SELECT id, org_id, block::text AS raw
        FROM deleted_blocks
        WHERE block::text ~ '"type"\s*:\s*"view"'
          AND block::text ~ key_pattern
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

        IF jsonb_typeof(doc) <> 'object'
           OR COALESCE(doc ->> 'type', '') <> 'view'
           OR COALESCE(jsonb_typeof(doc -> 'fields'), '') <> 'object'
        THEN
            CONTINUE;
        END IF;

        fields_obj := doc -> 'fields';
        changed := false;
        FOREACH pair SLICE 1 IN ARRAY pairs LOOP
            IF fields_obj ? pair[1] THEN
                IF NOT (fields_obj ? pair[2]) THEN
                    fields_obj := fields_obj || jsonb_build_object(pair[2], fields_obj -> pair[1]);
                END IF;
                fields_obj := fields_obj - pair[1];
                changed := true;
            END IF;
        END LOOP;

        IF NOT changed THEN
            CONTINUE;
        END IF;

        doc := jsonb_set(doc, '{fields}', fields_obj);
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
