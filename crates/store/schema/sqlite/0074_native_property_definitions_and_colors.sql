-- Native `CardPropertyDefinition` shape and lowercase `PropertyColor` values across
-- `containers.card_properties` and `deleted_containers.container` (`$.cardProperties`):
--   - Every definition is normalized to `{id, name, type, options}` with a closed
--     `PropertyType` (`text` fallback).
--   - Every option requires a non-empty unique `id` (keeping the first occurrence),
--     renames `value` -> `label`, and maps legacy `prop` + `Color*` names to
--     `"default" | "gray" | "brown" | "orange" | "yellow" | "green" | "blue" |
--     "purple" | "pink" | "red"` (`"default"` fallback).

UPDATE containers
SET card_properties = '[]'
WHERE CASE
    WHEN json_valid(card_properties) THEN json_type(card_properties) <> 'array'
    ELSE 1
END;

UPDATE containers
SET card_properties = (
    SELECT COALESCE(json_group_array(
        json_object(
            'id', CASE
                WHEN json_type(def.value, '$.id') = 'text' THEN json_extract(def.value, '$.id')
                ELSE ''
            END,
            'name', CASE
                WHEN json_type(def.value, '$.name') = 'text' THEN json_extract(def.value, '$.name')
                ELSE ''
            END,
            'type', CASE
                WHEN json_type(def.value, '$.type') = 'text'
                     AND json_extract(def.value, '$.type') IN (
                         'text', 'number', 'select', 'multiSelect', 'date',
                         'person', 'multiPerson', 'file', 'checkbox', 'url',
                         'email', 'phone', 'createdTime', 'createdBy',
                         'updatedTime', 'updatedBy', 'relation'
                     )
                THEN json_extract(def.value, '$.type')
                ELSE 'text'
            END,
            'options', json(CASE
                WHEN json_type(def.value, '$.options') = 'array' THEN (
                    SELECT COALESCE(json_group_array(
                        json_object(
                            'id', json_extract(opt.value, '$.id'),
                            'label', CASE
                                WHEN json_type(opt.value, '$.label') = 'text'
                                THEN json_extract(opt.value, '$.label')
                                WHEN json_type(opt.value, '$.value') = 'text'
                                THEN json_extract(opt.value, '$.value')
                                ELSE ''
                            END,
                            'color', CASE
                                WHEN json_type(opt.value, '$.color') <> 'text' THEN 'default'
                                ELSE CASE json_extract(opt.value, '$.color')
                                    WHEN 'prop' || 'ColorDefault' THEN 'default'
                                    WHEN 'prop' || 'ColorGray' THEN 'gray'
                                    WHEN 'prop' || 'ColorBrown' THEN 'brown'
                                    WHEN 'prop' || 'ColorOrange' THEN 'orange'
                                    WHEN 'prop' || 'ColorYellow' THEN 'yellow'
                                    WHEN 'prop' || 'ColorGreen' THEN 'green'
                                    WHEN 'prop' || 'ColorBlue' THEN 'blue'
                                    WHEN 'prop' || 'ColorPurple' THEN 'purple'
                                    WHEN 'prop' || 'ColorPink' THEN 'pink'
                                    WHEN 'prop' || 'ColorRed' THEN 'red'
                                    WHEN 'default' THEN 'default'
                                    WHEN 'gray' THEN 'gray'
                                    WHEN 'brown' THEN 'brown'
                                    WHEN 'orange' THEN 'orange'
                                    WHEN 'yellow' THEN 'yellow'
                                    WHEN 'green' THEN 'green'
                                    WHEN 'blue' THEN 'blue'
                                    WHEN 'purple' THEN 'purple'
                                    WHEN 'pink' THEN 'pink'
                                    WHEN 'red' THEN 'red'
                                    ELSE 'default'
                                END
                            END
                        ) ORDER BY opt.key
                    ), '[]')
                    FROM json_each(def.value, '$.options') AS opt
                    WHERE opt.type = 'object'
                      AND json_type(opt.value, '$.id') = 'text'
                      AND json_extract(opt.value, '$.id') <> ''
                      AND NOT EXISTS (
                          SELECT 1
                          FROM json_each(def.value, '$.options') AS earlier
                          WHERE earlier.type = 'object'
                            AND earlier.key < opt.key
                            AND json_type(earlier.value, '$.id') = 'text'
                            AND json_extract(earlier.value, '$.id') = json_extract(opt.value, '$.id')
                      )
                )
                ELSE '[]'
            END)
        ) ORDER BY def.key
    ), '[]')
    FROM json_each(containers.card_properties) AS def
    WHERE def.type = 'object'
)
WHERE CASE
    WHEN json_valid(card_properties) THEN
        json_type(card_properties) = 'array'
        AND json_array_length(card_properties) > 0
    ELSE 0
END;

UPDATE deleted_containers
SET container = json_set(container, '$.cardProperties', json('[]'))
WHERE CASE
    WHEN json_valid(container) THEN
        json_type(container) = 'object'
        AND json_type(container, '$.cardProperties') IS NOT NULL
        AND json_type(container, '$.cardProperties') <> 'array'
    ELSE 0
END;

UPDATE deleted_containers
SET container = json_set(
    container,
    '$.cardProperties',
    json((
        SELECT COALESCE(json_group_array(
            json_object(
                'id', CASE
                    WHEN json_type(def.value, '$.id') = 'text' THEN json_extract(def.value, '$.id')
                    ELSE ''
                END,
                'name', CASE
                    WHEN json_type(def.value, '$.name') = 'text' THEN json_extract(def.value, '$.name')
                    ELSE ''
                END,
                'type', CASE
                    WHEN json_type(def.value, '$.type') = 'text'
                         AND json_extract(def.value, '$.type') IN (
                             'text', 'number', 'select', 'multiSelect', 'date',
                             'person', 'multiPerson', 'file', 'checkbox', 'url',
                             'email', 'phone', 'createdTime', 'createdBy',
                             'updatedTime', 'updatedBy', 'relation'
                         )
                    THEN json_extract(def.value, '$.type')
                    ELSE 'text'
                END,
                'options', json(CASE
                    WHEN json_type(def.value, '$.options') = 'array' THEN (
                        SELECT COALESCE(json_group_array(
                            json_object(
                                'id', json_extract(opt.value, '$.id'),
                                'label', CASE
                                    WHEN json_type(opt.value, '$.label') = 'text'
                                    THEN json_extract(opt.value, '$.label')
                                    WHEN json_type(opt.value, '$.value') = 'text'
                                    THEN json_extract(opt.value, '$.value')
                                    ELSE ''
                                END,
                                'color', CASE
                                    WHEN json_type(opt.value, '$.color') <> 'text' THEN 'default'
                                    ELSE CASE json_extract(opt.value, '$.color')
                                        WHEN 'prop' || 'ColorDefault' THEN 'default'
                                        WHEN 'prop' || 'ColorGray' THEN 'gray'
                                        WHEN 'prop' || 'ColorBrown' THEN 'brown'
                                        WHEN 'prop' || 'ColorOrange' THEN 'orange'
                                        WHEN 'prop' || 'ColorYellow' THEN 'yellow'
                                        WHEN 'prop' || 'ColorGreen' THEN 'green'
                                        WHEN 'prop' || 'ColorBlue' THEN 'blue'
                                        WHEN 'prop' || 'ColorPurple' THEN 'purple'
                                        WHEN 'prop' || 'ColorPink' THEN 'pink'
                                        WHEN 'prop' || 'ColorRed' THEN 'red'
                                        WHEN 'default' THEN 'default'
                                        WHEN 'gray' THEN 'gray'
                                        WHEN 'brown' THEN 'brown'
                                        WHEN 'orange' THEN 'orange'
                                        WHEN 'yellow' THEN 'yellow'
                                        WHEN 'green' THEN 'green'
                                        WHEN 'blue' THEN 'blue'
                                        WHEN 'purple' THEN 'purple'
                                        WHEN 'pink' THEN 'pink'
                                        WHEN 'red' THEN 'red'
                                        ELSE 'default'
                                    END
                                END
                            ) ORDER BY opt.key
                        ), '[]')
                        FROM json_each(def.value, '$.options') AS opt
                        WHERE opt.type = 'object'
                          AND json_type(opt.value, '$.id') = 'text'
                          AND json_extract(opt.value, '$.id') <> ''
                          AND NOT EXISTS (
                              SELECT 1
                              FROM json_each(def.value, '$.options') AS earlier
                              WHERE earlier.type = 'object'
                                AND earlier.key < opt.key
                                AND json_type(earlier.value, '$.id') = 'text'
                                AND json_extract(earlier.value, '$.id') = json_extract(opt.value, '$.id')
                          )
                    )
                    ELSE '[]'
                END)
            ) ORDER BY def.key
        ), '[]')
        FROM json_each(deleted_containers.container, '$.cardProperties') AS def
        WHERE def.type = 'object'
    ))
)
WHERE CASE
    WHEN json_valid(container) THEN
        json_type(container) = 'object'
        AND json_type(container, '$.cardProperties') = 'array'
        AND json_array_length(container, '$.cardProperties') > 0
    ELSE 0
END;
