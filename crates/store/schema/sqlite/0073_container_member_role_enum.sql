-- Single `role` enum (`admin`, `editor`, `commenter`, `viewer`) on `container_members`,
-- `deleted_containers.members` snapshots, and `workspace_members.prior_container_roles`.

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
FROM container_members;

DROP TABLE container_members;
ALTER TABLE container_members_new RENAME TO container_members;
CREATE INDEX container_members_user_id_idx ON container_members (user_id);

UPDATE deleted_containers
SET members = (
    SELECT json_group_array(
        json(
            CASE
                WHEN json_type(m.value) = 'object' THEN json_set(
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
                ELSE m.value
            END
        )
        ORDER BY m.key
    )
    FROM json_each(deleted_containers.members) AS m
)
WHERE json_type(deleted_containers.members) = 'array'
  AND EXISTS (
      SELECT 1
      FROM json_each(deleted_containers.members) AS m
      WHERE json_type(m.value) = 'object'
        AND (
            json_type(m.value, '$.schemeAdmin') IS NOT NULL
            OR json_type(m.value, '$.schemeEditor') IS NOT NULL
            OR json_type(m.value, '$.schemeCommenter') IS NOT NULL
            OR json_type(m.value, '$.schemeViewer') IS NOT NULL
            OR json_type(m.value, '$.role') IS NULL
        )
  );

UPDATE workspace_members
SET prior_container_roles = (
    SELECT json_group_array(
        json(
            CASE
                WHEN json_type(e.value) = 'array'
                     AND json_type(e.value, '$[1]') = 'array'
                    THEN json_array(
                        json_extract(e.value, '$[0]'),
                        CASE
                            WHEN json_type(e.value, '$[1][0]') = 'true' THEN 'admin'
                            WHEN json_type(e.value, '$[1][1]') = 'true' THEN 'editor'
                            WHEN json_type(e.value, '$[1][2]') = 'true' THEN 'commenter'
                            ELSE 'viewer'
                        END
                    )
                ELSE e.value
            END
        )
        ORDER BY e.key
    )
    FROM json_each(workspace_members.prior_container_roles) AS e
)
WHERE prior_container_roles IS NOT NULL
  AND json_type(workspace_members.prior_container_roles) = 'array'
  AND EXISTS (
      SELECT 1
      FROM json_each(workspace_members.prior_container_roles) AS e
      WHERE json_type(e.value) = 'array'
        AND json_type(e.value, '$[1]') = 'array'
  );
