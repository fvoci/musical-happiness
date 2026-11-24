-- Native view `fields` keys across `blocks` (`type = 'view'`) and `deleted_blocks`
-- (`$.type = 'view'`):
--   visibleOptionIds      -> visibleGroups
--   hiddenOptionIds       -> hiddenGroups
--   collapsedOptionIds    -> collapsedGroups
--   kanbanCalculations    -> groupCalculations
--   visiblePropertyIds    -> shownProperties
--   groupById             -> groupBy
--   sortOptions           -> sort
--   dateDisplayPropertyId -> dateProperty

WITH
s0 AS MATERIALIZED (
    SELECT id, fields AS f
    FROM blocks
    WHERE type = 'view'
      AND json_type(fields) = 'object'
      AND (
          json_type(fields, '$.visibleOptionIds') IS NOT NULL
          OR json_type(fields, '$.hiddenOptionIds') IS NOT NULL
          OR json_type(fields, '$.collapsedOptionIds') IS NOT NULL
          OR json_type(fields, '$.kanbanCalculations') IS NOT NULL
          OR json_type(fields, '$.visiblePropertyIds') IS NOT NULL
          OR json_type(fields, '$.groupById') IS NOT NULL
          OR json_type(fields, '$.sortOptions') IS NOT NULL
          OR json_type(fields, '$.dateDisplayPropertyId') IS NOT NULL
      )
),
s1 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.visibleOptionIds') IS NULL THEN f
        WHEN json_type(f, '$.visibleGroups') IS NULL
            THEN json_set(json_remove(f, '$.visibleOptionIds'), '$.visibleGroups', json(f -> '$.visibleOptionIds'))
        ELSE json_remove(f, '$.visibleOptionIds')
    END AS f
    FROM s0
),
s2 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.hiddenOptionIds') IS NULL THEN f
        WHEN json_type(f, '$.hiddenGroups') IS NULL
            THEN json_set(json_remove(f, '$.hiddenOptionIds'), '$.hiddenGroups', json(f -> '$.hiddenOptionIds'))
        ELSE json_remove(f, '$.hiddenOptionIds')
    END AS f
    FROM s1
),
s3 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.collapsedOptionIds') IS NULL THEN f
        WHEN json_type(f, '$.collapsedGroups') IS NULL
            THEN json_set(json_remove(f, '$.collapsedOptionIds'), '$.collapsedGroups', json(f -> '$.collapsedOptionIds'))
        ELSE json_remove(f, '$.collapsedOptionIds')
    END AS f
    FROM s2
),
s4 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.kanbanCalculations') IS NULL THEN f
        WHEN json_type(f, '$.groupCalculations') IS NULL
            THEN json_set(json_remove(f, '$.kanbanCalculations'), '$.groupCalculations', json(f -> '$.kanbanCalculations'))
        ELSE json_remove(f, '$.kanbanCalculations')
    END AS f
    FROM s3
),
s5 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.visiblePropertyIds') IS NULL THEN f
        WHEN json_type(f, '$.shownProperties') IS NULL
            THEN json_set(json_remove(f, '$.visiblePropertyIds'), '$.shownProperties', json(f -> '$.visiblePropertyIds'))
        ELSE json_remove(f, '$.visiblePropertyIds')
    END AS f
    FROM s4
),
s6 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.groupById') IS NULL THEN f
        WHEN json_type(f, '$.groupBy') IS NULL
            THEN json_set(json_remove(f, '$.groupById'), '$.groupBy', json(f -> '$.groupById'))
        ELSE json_remove(f, '$.groupById')
    END AS f
    FROM s5
),
s7 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.sortOptions') IS NULL THEN f
        WHEN json_type(f, '$.sort') IS NULL
            THEN json_set(json_remove(f, '$.sortOptions'), '$.sort', json(f -> '$.sortOptions'))
        ELSE json_remove(f, '$.sortOptions')
    END AS f
    FROM s6
),
s8 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.dateDisplayPropertyId') IS NULL THEN f
        WHEN json_type(f, '$.dateProperty') IS NULL
            THEN json_set(json_remove(f, '$.dateDisplayPropertyId'), '$.dateProperty', json(f -> '$.dateDisplayPropertyId'))
        ELSE json_remove(f, '$.dateDisplayPropertyId')
    END AS f
    FROM s7
)
UPDATE blocks
SET fields = s8.f
FROM s8
WHERE blocks.id = s8.id;

WITH
s0 AS MATERIALIZED (
    SELECT id, block -> '$.fields' AS f
    FROM deleted_blocks
    WHERE json_type(block) = 'object'
      AND json_extract(block, '$.type') = 'view'
      AND json_type(block, '$.fields') = 'object'
      AND (
          json_type(block, '$.fields.visibleOptionIds') IS NOT NULL
          OR json_type(block, '$.fields.hiddenOptionIds') IS NOT NULL
          OR json_type(block, '$.fields.collapsedOptionIds') IS NOT NULL
          OR json_type(block, '$.fields.kanbanCalculations') IS NOT NULL
          OR json_type(block, '$.fields.visiblePropertyIds') IS NOT NULL
          OR json_type(block, '$.fields.groupById') IS NOT NULL
          OR json_type(block, '$.fields.sortOptions') IS NOT NULL
          OR json_type(block, '$.fields.dateDisplayPropertyId') IS NOT NULL
      )
),
s1 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.visibleOptionIds') IS NULL THEN f
        WHEN json_type(f, '$.visibleGroups') IS NULL
            THEN json_set(json_remove(f, '$.visibleOptionIds'), '$.visibleGroups', json(f -> '$.visibleOptionIds'))
        ELSE json_remove(f, '$.visibleOptionIds')
    END AS f
    FROM s0
),
s2 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.hiddenOptionIds') IS NULL THEN f
        WHEN json_type(f, '$.hiddenGroups') IS NULL
            THEN json_set(json_remove(f, '$.hiddenOptionIds'), '$.hiddenGroups', json(f -> '$.hiddenOptionIds'))
        ELSE json_remove(f, '$.hiddenOptionIds')
    END AS f
    FROM s1
),
s3 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.collapsedOptionIds') IS NULL THEN f
        WHEN json_type(f, '$.collapsedGroups') IS NULL
            THEN json_set(json_remove(f, '$.collapsedOptionIds'), '$.collapsedGroups', json(f -> '$.collapsedOptionIds'))
        ELSE json_remove(f, '$.collapsedOptionIds')
    END AS f
    FROM s2
),
s4 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.kanbanCalculations') IS NULL THEN f
        WHEN json_type(f, '$.groupCalculations') IS NULL
            THEN json_set(json_remove(f, '$.kanbanCalculations'), '$.groupCalculations', json(f -> '$.kanbanCalculations'))
        ELSE json_remove(f, '$.kanbanCalculations')
    END AS f
    FROM s3
),
s5 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.visiblePropertyIds') IS NULL THEN f
        WHEN json_type(f, '$.shownProperties') IS NULL
            THEN json_set(json_remove(f, '$.visiblePropertyIds'), '$.shownProperties', json(f -> '$.visiblePropertyIds'))
        ELSE json_remove(f, '$.visiblePropertyIds')
    END AS f
    FROM s4
),
s6 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.groupById') IS NULL THEN f
        WHEN json_type(f, '$.groupBy') IS NULL
            THEN json_set(json_remove(f, '$.groupById'), '$.groupBy', json(f -> '$.groupById'))
        ELSE json_remove(f, '$.groupById')
    END AS f
    FROM s5
),
s7 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.sortOptions') IS NULL THEN f
        WHEN json_type(f, '$.sort') IS NULL
            THEN json_set(json_remove(f, '$.sortOptions'), '$.sort', json(f -> '$.sortOptions'))
        ELSE json_remove(f, '$.sortOptions')
    END AS f
    FROM s6
),
s8 AS MATERIALIZED (
    SELECT id, CASE
        WHEN json_type(f, '$.dateDisplayPropertyId') IS NULL THEN f
        WHEN json_type(f, '$.dateProperty') IS NULL
            THEN json_set(json_remove(f, '$.dateDisplayPropertyId'), '$.dateProperty', json(f -> '$.dateDisplayPropertyId'))
        ELSE json_remove(f, '$.dateDisplayPropertyId')
    END AS f
    FROM s7
)
UPDATE deleted_blocks
SET block = json_set(deleted_blocks.block, '$.fields', json(s8.f))
FROM s8
WHERE deleted_blocks.id = s8.id;
