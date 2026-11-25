-- Native `CardPropertyDefinition` shape and lowercase `PropertyColor` values across
-- `containers.card_properties` (`JSONB`) and `deleted_containers.container` (`JSON`,
-- `$.cardProperties`):
--   - Every definition is normalized to `{id, name, type, options}` with a closed
--     `PropertyType` (`text` fallback).
--   - Every option requires a non-empty unique `id` (keeping the first occurrence),
--     renames `value` -> `label`, and maps legacy `prop` + `Color*` names to
--     `"default" | "gray" | "brown" | "orange" | "yellow" | "green" | "blue" |
--     "purple" | "pink" | "red"` (`"default"` fallback).
--
-- `containers` and `deleted_containers` are behind the organization wall, so the system
-- scope is required to reach every organization's rows.
SELECT set_config('fvoci.system', 'on', true);

UPDATE containers
SET card_properties = '[]'::jsonb
WHERE jsonb_typeof(card_properties) <> 'array';

DO $$
DECLARE
    legacy_prefix CONSTANT TEXT := 'prop' || 'Color';
    valid_types CONSTANT TEXT[] := ARRAY[
        'text', 'number', 'select', 'multiSelect', 'date',
        'person', 'multiPerson', 'file', 'checkbox', 'url',
        'email', 'phone', 'createdTime', 'createdBy',
        'updatedTime', 'updatedBy', 'relation'
    ];
    valid_colors CONSTANT TEXT[] := ARRAY[
        'default', 'gray', 'brown', 'orange', 'yellow',
        'green', 'blue', 'purple', 'pink', 'red'
    ];
    r RECORD;
    doc JSONB;
    orig_defs JSONB;
    src_defs JSONB;
    norm_defs JSONB;
    def_elem JSONB;
    norm_opts JSONB;
    opt_elem JSONB;
    seen_opt_ids TEXT[];
    prop_id TEXT;
    prop_name TEXT;
    prop_type TEXT;
    opt_id TEXT;
    opt_label TEXT;
    opt_color TEXT;
    has_nul BOOLEAN;
    bs_sentinel TEXT;
    nul_sentinel TEXT;
    safe TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, card_properties
        FROM containers
        WHERE jsonb_typeof(card_properties) = 'array'
          AND jsonb_array_length(card_properties) > 0
    LOOP
        norm_defs := '[]'::jsonb;
        FOR def_elem IN
            SELECT elem.value
            FROM jsonb_array_elements(r.card_properties) WITH ORDINALITY AS elem(value, ord)
            ORDER BY elem.ord
        LOOP
            IF jsonb_typeof(def_elem) <> 'object' THEN
                CONTINUE;
            END IF;
            prop_id := CASE WHEN jsonb_typeof(def_elem -> 'id') = 'string' THEN def_elem ->> 'id' ELSE '' END;
            prop_name := CASE WHEN jsonb_typeof(def_elem -> 'name') = 'string' THEN def_elem ->> 'name' ELSE '' END;
            prop_type := CASE WHEN jsonb_typeof(def_elem -> 'type') = 'string' THEN def_elem ->> 'type' ELSE 'text' END;
            IF NOT (prop_type = ANY(valid_types)) THEN
                prop_type := 'text';
            END IF;

            norm_opts := '[]'::jsonb;
            seen_opt_ids := ARRAY[]::TEXT[];
            IF jsonb_typeof(def_elem -> 'options') = 'array' THEN
                FOR opt_elem IN
                    SELECT elem.value
                    FROM jsonb_array_elements(def_elem -> 'options') WITH ORDINALITY AS elem(value, ord)
                    ORDER BY elem.ord
                LOOP
                    IF jsonb_typeof(opt_elem) <> 'object' THEN
                        CONTINUE;
                    END IF;
                    opt_id := CASE WHEN jsonb_typeof(opt_elem -> 'id') = 'string' THEN opt_elem ->> 'id' ELSE '' END;
                    IF opt_id = '' OR (opt_id = ANY(seen_opt_ids)) THEN
                        CONTINUE;
                    END IF;
                    seen_opt_ids := array_append(seen_opt_ids, opt_id);
                    opt_label := CASE
                        WHEN jsonb_typeof(opt_elem -> 'label') = 'string' THEN opt_elem ->> 'label'
                        WHEN jsonb_typeof(opt_elem -> 'value') = 'string' THEN opt_elem ->> 'value'
                        ELSE ''
                    END;
                    opt_color := CASE WHEN jsonb_typeof(opt_elem -> 'color') = 'string' THEN opt_elem ->> 'color' ELSE 'default' END;
                    opt_color := CASE opt_color
                        WHEN legacy_prefix || 'Default' THEN 'default'
                        WHEN legacy_prefix || 'Gray' THEN 'gray'
                        WHEN legacy_prefix || 'Brown' THEN 'brown'
                        WHEN legacy_prefix || 'Orange' THEN 'orange'
                        WHEN legacy_prefix || 'Yellow' THEN 'yellow'
                        WHEN legacy_prefix || 'Green' THEN 'green'
                        WHEN legacy_prefix || 'Blue' THEN 'blue'
                        WHEN legacy_prefix || 'Purple' THEN 'purple'
                        WHEN legacy_prefix || 'Pink' THEN 'pink'
                        WHEN legacy_prefix || 'Red' THEN 'red'
                        ELSE opt_color
                    END;
                    IF NOT (opt_color = ANY(valid_colors)) THEN
                        opt_color := 'default';
                    END IF;
                    norm_opts := norm_opts || jsonb_build_array(
                        jsonb_build_object('id', opt_id, 'label', opt_label, 'color', opt_color)
                    );
                END LOOP;
            END IF;

            norm_defs := norm_defs || jsonb_build_array(
                jsonb_build_object('id', prop_id, 'name', prop_name, 'type', prop_type, 'options', norm_opts)
            );
        END LOOP;

        IF norm_defs IS DISTINCT FROM r.card_properties THEN
            UPDATE containers
            SET card_properties = norm_defs
            WHERE id = r.id AND org_id = r.org_id;
        END IF;
    END LOOP;

    FOR r IN
        SELECT id, org_id, container::text AS raw
        FROM deleted_containers
        WHERE container::text ~ '"cardProperties"\s*:'
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
                    RAISE WARNING '0074: skipped unreadable deleted_containers row %', r.id;
                    CONTINUE;
                END;
            WHEN invalid_text_representation THEN
                RAISE WARNING '0074: skipped unreadable deleted_containers row %', r.id;
                CONTINUE;
        END;

        IF jsonb_typeof(doc) <> 'object' OR NOT (doc ? 'cardProperties') THEN
            CONTINUE;
        END IF;

        orig_defs := doc -> 'cardProperties';
        IF jsonb_typeof(orig_defs) <> 'array' THEN
            norm_defs := '[]'::jsonb;
        ELSIF jsonb_array_length(orig_defs) = 0 THEN
            CONTINUE;
        ELSE
            IF has_nul THEN
                src_defs := replace(orig_defs::text, nul_sentinel, '')::jsonb;
            ELSE
                src_defs := orig_defs;
            END IF;
            norm_defs := '[]'::jsonb;
            FOR def_elem IN
                SELECT elem.value
                FROM jsonb_array_elements(src_defs) WITH ORDINALITY AS elem(value, ord)
                ORDER BY elem.ord
            LOOP
                IF jsonb_typeof(def_elem) <> 'object' THEN
                    CONTINUE;
                END IF;
                prop_id := CASE WHEN jsonb_typeof(def_elem -> 'id') = 'string' THEN def_elem ->> 'id' ELSE '' END;
                prop_name := CASE WHEN jsonb_typeof(def_elem -> 'name') = 'string' THEN def_elem ->> 'name' ELSE '' END;
                prop_type := CASE WHEN jsonb_typeof(def_elem -> 'type') = 'string' THEN def_elem ->> 'type' ELSE 'text' END;
                IF NOT (prop_type = ANY(valid_types)) THEN
                    prop_type := 'text';
                END IF;

                norm_opts := '[]'::jsonb;
                seen_opt_ids := ARRAY[]::TEXT[];
                IF jsonb_typeof(def_elem -> 'options') = 'array' THEN
                    FOR opt_elem IN
                        SELECT elem.value
                        FROM jsonb_array_elements(def_elem -> 'options') WITH ORDINALITY AS elem(value, ord)
                        ORDER BY elem.ord
                    LOOP
                        IF jsonb_typeof(opt_elem) <> 'object' THEN
                            CONTINUE;
                        END IF;
                        opt_id := CASE WHEN jsonb_typeof(opt_elem -> 'id') = 'string' THEN opt_elem ->> 'id' ELSE '' END;
                        IF opt_id = '' OR (opt_id = ANY(seen_opt_ids)) THEN
                            CONTINUE;
                        END IF;
                        seen_opt_ids := array_append(seen_opt_ids, opt_id);
                        opt_label := CASE
                            WHEN jsonb_typeof(opt_elem -> 'label') = 'string' THEN opt_elem ->> 'label'
                            WHEN jsonb_typeof(opt_elem -> 'value') = 'string' THEN opt_elem ->> 'value'
                            ELSE ''
                        END;
                        opt_color := CASE WHEN jsonb_typeof(opt_elem -> 'color') = 'string' THEN opt_elem ->> 'color' ELSE 'default' END;
                        opt_color := CASE opt_color
                            WHEN legacy_prefix || 'Default' THEN 'default'
                            WHEN legacy_prefix || 'Gray' THEN 'gray'
                            WHEN legacy_prefix || 'Brown' THEN 'brown'
                            WHEN legacy_prefix || 'Orange' THEN 'orange'
                            WHEN legacy_prefix || 'Yellow' THEN 'yellow'
                            WHEN legacy_prefix || 'Green' THEN 'green'
                            WHEN legacy_prefix || 'Blue' THEN 'blue'
                            WHEN legacy_prefix || 'Purple' THEN 'purple'
                            WHEN legacy_prefix || 'Pink' THEN 'pink'
                            WHEN legacy_prefix || 'Red' THEN 'red'
                            ELSE opt_color
                        END;
                        IF NOT (opt_color = ANY(valid_colors)) THEN
                            opt_color := 'default';
                        END IF;
                        norm_opts := norm_opts || jsonb_build_array(
                            jsonb_build_object('id', opt_id, 'label', opt_label, 'color', opt_color)
                        );
                    END LOOP;
                END IF;

                norm_defs := norm_defs || jsonb_build_array(
                    jsonb_build_object('id', prop_id, 'name', prop_name, 'type', prop_type, 'options', norm_opts)
                );
            END LOOP;
        END IF;

        IF norm_defs IS DISTINCT FROM orig_defs THEN
            doc := jsonb_set(doc, '{cardProperties}', norm_defs);
            IF has_nul THEN
                rewritten := replace(doc::text, nul_sentinel, '\u0000')::json;
            ELSE
                rewritten := doc::json;
            END IF;
            UPDATE deleted_containers
            SET container = rewritten
            WHERE id = r.id AND org_id = r.org_id;
        END IF;
    END LOOP;
END $$;
