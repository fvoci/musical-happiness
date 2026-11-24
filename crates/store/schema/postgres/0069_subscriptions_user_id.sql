-- Subscriptions belong to users alone: drop `subscriber_type` (after removing any
-- non-user rows) and rename `subscriber_id` to `user_id`, both in `subscriptions` and
-- in the saved `watchedBy` arrays of `deleted_blocks`.
--
-- Both `subscriptions` and `deleted_blocks` are behind the organization wall, so the
-- system scope is required to reach every organization's rows during migration.
SELECT set_config('fvoci.system', 'on', true);

DELETE FROM subscriptions WHERE subscriber_type <> 'user';

ALTER TABLE subscriptions DROP COLUMN subscriber_type;
ALTER TABLE subscriptions RENAME COLUMN subscriber_id TO user_id;

DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace
            AND c.conrelid = 'subscriptions'::regclass
            AND c.conname LIKE '%subscriber_id%'
    LOOP
        EXECUTE format(
            'ALTER TABLE subscriptions RENAME CONSTRAINT %I TO %I',
            found.name,
            replace(found.name, 'subscriber_id', 'user_id')
        );
    END LOOP;
END $$;

DROP INDEX subscriptions_subscriber_id_idx;
CREATE INDEX subscriptions_user_id_idx ON subscriptions (user_id);

DO $$
DECLARE
    r RECORD;
    doc JSONB;
    kept_jsonb JSONB;
    escaped_kept TEXT;
    rewritten JSON;
BEGIN
    FOR r IN
        SELECT id, org_id, block::text AS raw
        FROM deleted_blocks
        WHERE block::text ~ '"watchedBy"\s*:\s*\['
    LOOP
        BEGIN
            doc := r.raw::jsonb;
            IF COALESCE(jsonb_typeof(doc -> 'watchedBy'), '') <> 'array' THEN
                CONTINUE;
            END IF;

            SELECT jsonb_agg(
                jsonb_build_object(
                    'blockType', w.elem -> 'blockType',
                    'createAt', w.elem -> 'createAt',
                    'notifiedAt', w.elem -> 'notifiedAt',
                    'userId', w.elem -> 'subscriberId'
                )
                ORDER BY w.ord
            )
            INTO kept_jsonb
            FROM jsonb_array_elements(doc -> 'watchedBy') WITH ORDINALITY AS w(elem, ord)
            WHERE w.elem ->> 'subscriberType' = 'user';

            IF kept_jsonb IS NULL THEN
                rewritten := (doc - 'watchedBy')::json;
            ELSE
                rewritten := jsonb_set(doc, '{watchedBy}', kept_jsonb)::json;
            END IF;
        EXCEPTION WHEN untranslatable_character THEN
            -- `jsonb` and `json -> key` refuse `\u0000` kept by `json` in `fields` or `body`.
            -- Probe structure with `\u0000` substituted so nested `fields.watchedBy` is never
            -- mistaken for a top-level key, then splice only top-level `"watchedBy"` in `r.raw`.
            doc := replace(r.raw, '\u0000', '\ufffd')::jsonb;
            IF COALESCE(jsonb_typeof(doc -> 'watchedBy'), '') <> 'array' THEN
                CONTINUE;
            END IF;

            SELECT jsonb_agg(
                jsonb_build_object(
                    'blockType', w.elem -> 'blockType',
                    'createAt', w.elem -> 'createAt',
                    'notifiedAt', w.elem -> 'notifiedAt',
                    'userId', w.elem -> 'subscriberId'
                )
                ORDER BY w.ord
            )
            INTO kept_jsonb
            FROM jsonb_array_elements(doc -> 'watchedBy') WITH ORDINALITY AS w(elem, ord)
            WHERE w.elem ->> 'subscriberType' = 'user';

            IF kept_jsonb IS NULL THEN
                IF r.raw ~ '^\s*\{\s*"watchedBy"\s*:' THEN
                    rewritten := regexp_replace(
                        r.raw,
                        '^(\s*\{\s*)"watchedBy"\s*:\s*\[[^\]]*\]\s*,\s*',
                        '\1'
                    )::json;
                ELSE
                    rewritten := regexp_replace(
                        r.raw,
                        '^(.*),\s*"watchedBy"\s*:\s*\[.*\](\s*\}\s*)$',
                        '\1\2'
                    )::json;
                END IF;
            ELSE
                escaped_kept := replace(kept_jsonb::text, '\', '\\');
                IF r.raw ~ '^\s*\{\s*"watchedBy"\s*:' THEN
                    rewritten := regexp_replace(
                        r.raw,
                        '^(\s*\{\s*)"watchedBy"\s*:\s*\[[^\]]*\]',
                        '\1"watchedBy":' || escaped_kept
                    )::json;
                ELSE
                    rewritten := regexp_replace(
                        r.raw,
                        '^(.*)"watchedBy"\s*:\s*\[.*\](\s*\}\s*)$',
                        '\1"watchedBy":' || escaped_kept || '\2'
                    )::json;
                END IF;
            END IF;
        END;

        UPDATE deleted_blocks
        SET block = rewritten
        WHERE id = r.id AND org_id = r.org_id;
    END LOOP;
END $$;
