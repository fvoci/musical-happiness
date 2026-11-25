-- Single `role` enum (`admin`, `editor`, `commenter`, `viewer`) on `container_members`,
-- `deleted_containers.members` snapshots, and `workspace_members.prior_container_roles`.
-- Legacy rows and snapshot entries where every `scheme_*` flag was false held no role
-- and are pruned rather than elevated to `viewer`.

CREATE TABLE container_members_new (
    container_id TEXT NOT NULL REFERENCES containers (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'editor', 'commenter', 'viewer')),
    PRIMARY KEY (container_id, user_id)
);

INSERT INTO container_members_new (container_id, user_id, role)
SELECT
    container_id,
    user_id,
    CASE
        WHEN scheme_admin THEN 'admin'
        WHEN scheme_editor THEN 'editor'
        WHEN scheme_commenter THEN 'commenter'
        ELSE 'viewer'
    END
FROM container_members
WHERE COALESCE(scheme_admin, 0)
   OR COALESCE(scheme_editor, 0)
   OR COALESCE(scheme_commenter, 0)
   OR COALESCE(scheme_viewer, 0);

DROP TABLE container_members;
ALTER TABLE container_members_new RENAME TO container_members;
CREATE INDEX container_members_user_id_idx ON container_members (user_id);

UPDATE deleted_containers
SET members = COALESCE(
    (
        SELECT json_group_array(
            CASE
                WHEN m.type = 'object' THEN json(
                    json_set(
                        json_remove(
                            m.value,
                            '$.schemeAdmin',
                            '$.schemeEditor',
                            '$.schemeCommenter',
                            '$.schemeViewer'
                        ),
                        '$.role',
                        CASE
                            WHEN json_type(m.value, '$.role') = 'text'
                                 AND json_extract(m.value, '$.role') IN ('admin', 'editor', 'commenter', 'viewer')
                                THEN json_extract(m.value, '$.role')
                            WHEN json_type(m.value, '$.schemeAdmin') = 'true' THEN 'admin'
                            WHEN json_type(m.value, '$.schemeEditor') = 'true' THEN 'editor'
                            WHEN json_type(m.value, '$.schemeCommenter') = 'true' THEN 'commenter'
                            ELSE 'viewer'
                        END
                    )
                )
                WHEN m.type = 'text' THEN m.value
                WHEN m.type = 'true' THEN json('true')
                WHEN m.type = 'false' THEN json('false')
                WHEN m.type = 'null' THEN NULL
                ELSE json(m.value)
            END
            ORDER BY m.key
        )
        FROM json_each(deleted_containers.members) AS m
        WHERE NOT (
            m.type = 'object'
            AND NOT (
                json_type(m.value, '$.role') = 'text'
                AND json_extract(m.value, '$.role') IN ('admin', 'editor', 'commenter', 'viewer')
            )
            AND json_type(m.value, '$.schemeAdmin') IS NOT 'true'
            AND json_type(m.value, '$.schemeEditor') IS NOT 'true'
            AND json_type(m.value, '$.schemeCommenter') IS NOT 'true'
            AND json_type(m.value, '$.schemeViewer') IS NOT 'true'
        )
    ),
    '[]'
)
WHERE CASE
    WHEN json_valid(deleted_containers.members) THEN
        json_type(deleted_containers.members) = 'array'
        AND EXISTS (
            SELECT 1
            FROM json_each(deleted_containers.members) AS m
            WHERE m.type = 'object'
              AND (
                  json_type(m.value, '$.schemeAdmin') IS NOT NULL
                  OR json_type(m.value, '$.schemeEditor') IS NOT NULL
                  OR json_type(m.value, '$.schemeCommenter') IS NOT NULL
                  OR json_type(m.value, '$.schemeViewer') IS NOT NULL
                  OR NOT (
                      json_type(m.value, '$.role') = 'text'
                      AND json_extract(m.value, '$.role') IN ('admin', 'editor', 'commenter', 'viewer')
                  )
              )
        )
    ELSE 0
END;

UPDATE workspace_members
SET prior_container_roles = COALESCE(
    (
        SELECT json_group_array(
            CASE
                WHEN e.type = 'array'
                     AND json_type(e.value, '$[1]') = 'array'
                    THEN json(
                        json_array(
                            json_extract(e.value, '$[0]'),
                            CASE
                                WHEN json_type(e.value, '$[1][0]') = 'true' THEN 'admin'
                                WHEN json_type(e.value, '$[1][1]') = 'true' THEN 'editor'
                                WHEN json_type(e.value, '$[1][2]') = 'true' THEN 'commenter'
                                ELSE 'viewer'
                            END
                        )
                    )
                WHEN e.type = 'text' THEN e.value
                WHEN e.type = 'true' THEN json('true')
                WHEN e.type = 'false' THEN json('false')
                WHEN e.type = 'null' THEN NULL
                ELSE json(e.value)
            END
            ORDER BY e.key
        )
        FROM json_each(workspace_members.prior_container_roles) AS e
        WHERE NOT (
            e.type = 'array'
            AND json_type(e.value, '$[1]') = 'array'
            AND json_type(e.value, '$[1][0]') IS NOT 'true'
            AND json_type(e.value, '$[1][1]') IS NOT 'true'
            AND json_type(e.value, '$[1][2]') IS NOT 'true'
            AND json_type(e.value, '$[1][3]') IS NOT 'true'
        )
    ),
    '[]'
)
WHERE prior_container_roles IS NOT NULL
  AND CASE
      WHEN json_valid(workspace_members.prior_container_roles) THEN
          json_type(workspace_members.prior_container_roles) = 'array'
          AND EXISTS (
              SELECT 1
              FROM json_each(workspace_members.prior_container_roles) AS e
              WHERE e.type = 'array'
                AND json_type(e.value, '$[1]') = 'array'
          )
      ELSE 0
  END;
