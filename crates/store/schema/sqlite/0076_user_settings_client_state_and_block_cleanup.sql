-- Dedicated `user_settings` and `user_client_state` global tables (retiring `preferences`),
-- removal of `blocks."schema"` and `deleted_blocks.block` `schema`, and `organizations.kind`
-- `'shared'` (replacing `'team'`).

CREATE TABLE user_settings (
    user_id TEXT NOT NULL PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    language TEXT NOT NULL DEFAULT 'auto',
    theme TEXT NOT NULL DEFAULT 'auto' CHECK (theme IN ('auto', 'light', 'dark')),
    density TEXT NOT NULL DEFAULT 'auto' CHECK (density IN ('auto', 'compact', 'comfortable')),
    card_open TEXT NOT NULL DEFAULT 'center' CHECK (card_open IN ('center', 'side', 'full')),
    haptics BOOLEAN NOT NULL DEFAULT TRUE,
    week_start TEXT NOT NULL DEFAULT 'auto'
        CHECK (week_start IN ('auto', 'sunday', 'monday', 'saturday')),
    hour_cycle TEXT NOT NULL DEFAULT 'auto' CHECK (hour_cycle IN ('auto', 'h12', 'h23')),
    time_zone TEXT NOT NULL DEFAULT 'auto',
    notify_mentions BOOLEAN NOT NULL DEFAULT TRUE,
    notify_assignments BOOLEAN NOT NULL DEFAULT TRUE,
    notify_comments BOOLEAN NOT NULL DEFAULT TRUE,
    notify_reviews BOOLEAN NOT NULL DEFAULT TRUE,
    notify_meetings BOOLEAN NOT NULL DEFAULT TRUE,
    notify_due BOOLEAN NOT NULL DEFAULT TRUE
);

INSERT INTO user_settings (
    user_id,
    language,
    theme,
    density,
    card_open,
    haptics,
    week_start,
    hour_cycle,
    time_zone,
    notify_mentions,
    notify_assignments,
    notify_comments
)
SELECT
    p.user_id,
    COALESCE(MAX(CASE WHEN p.name = 'language' AND p.value <> '' THEN p.value END), 'auto'),
    COALESCE(
        MAX(CASE WHEN p.name = 'theme' AND p.value IN ('auto', 'light', 'dark') THEN p.value END),
        'auto'
    ),
    COALESCE(
        MAX(
            CASE
                WHEN p.name = 'density' AND p.value IN ('auto', 'compact', 'comfortable')
                    THEN p.value
            END
        ),
        'auto'
    ),
    COALESCE(
        MAX(
            CASE
                WHEN p.name = 'cardOpen' AND p.value IN ('center', 'side', 'full') THEN p.value
            END
        ),
        'center'
    ),
    CASE COALESCE(
        MAX(CASE WHEN p.name = 'haptics' AND p.value IN ('true', 'false') THEN p.value END),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END,
    COALESCE(
        MAX(
            CASE
                WHEN p.name = 'weekStart'
                     AND p.value IN ('auto', 'sunday', 'monday', 'saturday')
                    THEN p.value
            END
        ),
        'auto'
    ),
    COALESCE(
        MAX(CASE WHEN p.name = 'hourCycle' AND p.value IN ('auto', 'h12', 'h23') THEN p.value END),
        'auto'
    ),
    COALESCE(MAX(CASE WHEN p.name = 'timeZone' AND p.value <> '' THEN p.value END), 'auto'),
    CASE COALESCE(
        MAX(CASE WHEN p.name = 'notifyMentions' AND p.value IN ('true', 'false') THEN p.value END),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END,
    CASE COALESCE(
        MAX(
            CASE
                WHEN p.name = 'notifyAssignments' AND p.value IN ('true', 'false') THEN p.value
            END
        ),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END,
    CASE COALESCE(
        MAX(CASE WHEN p.name = 'notifyComments' AND p.value IN ('true', 'false') THEN p.value END),
        'true'
    )
        WHEN 'false' THEN FALSE
        ELSE TRUE
    END
FROM preferences AS p
WHERE p.category = 'settings'
GROUP BY p.user_id;

CREATE TABLE user_client_state (
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
);

INSERT INTO user_client_state (user_id, key, value)
SELECT user_id, name, value
FROM preferences
WHERE category = 'client';

DROP TABLE preferences;

ALTER TABLE blocks DROP COLUMN "schema";

UPDATE subscriptions
SET block_type = (SELECT b.type FROM blocks AS b WHERE b.id = subscriptions.block_id)
WHERE block_type NOT IN ('card', 'view', 'comment', 'attachment', 'page', 'entry', 'unit', 'chapter');

UPDATE deleted_blocks
SET block = json_remove(block, '$.schema')
WHERE CASE
    WHEN json_valid(block) THEN
        json_type(block) = 'object'
        AND json_type(block, '$.schema') IS NOT NULL
    ELSE 0
END;

DROP INDEX organizations_of_people_idx;
DROP INDEX organizations_owner_id_key;
DROP TRIGGER organizations_only_one;
DROP TRIGGER organizations_keep_their_kind;
DROP TRIGGER organization_members_of_a_space;
DROP TRIGGER organization_members_stay_of_a_space;
DROP TRIGGER workspace_members_of_a_space;
DROP TRIGGER workspace_members_stay_of_a_space;
DROP TRIGGER invitations_of_a_space;
DROP TRIGGER invitations_stay_out_of_a_space;
DROP TRIGGER diaries_of_a_space;
DROP TRIGGER diaries_stay_of_a_space;

ALTER TABLE organizations ADD COLUMN kind_new TEXT NOT NULL DEFAULT 'shared'
    CHECK (kind_new IN ('shared', 'personal'));
ALTER TABLE organizations ADD COLUMN owner_id_new TEXT REFERENCES users (id)
    CHECK ((kind_new = 'personal') = (owner_id_new IS NOT NULL)
           AND (NOT is_system OR kind_new = 'shared'));

UPDATE organizations
SET kind_new = CASE WHEN kind = 'team' THEN 'shared' ELSE kind END,
    owner_id_new = owner_id;

ALTER TABLE organizations DROP COLUMN owner_id;
ALTER TABLE organizations DROP COLUMN kind;

ALTER TABLE organizations RENAME COLUMN kind_new TO kind;
ALTER TABLE organizations RENAME COLUMN owner_id_new TO owner_id;

CREATE UNIQUE INDEX organizations_owner_id_key ON organizations (owner_id)
    WHERE owner_id IS NOT NULL;
CREATE INDEX organizations_of_people_idx ON organizations (created_at, id)
    WHERE kind = 'shared';

CREATE TRIGGER organizations_only_one BEFORE INSERT ON organizations
WHEN NOT NEW.is_system AND NEW.kind = 'shared'
     AND EXISTS (SELECT 1 FROM organizations WHERE NOT is_system AND kind = 'shared')
BEGIN
    SELECT RAISE(ABORT, 'an SQLite database holds one organization');
END;

CREATE TRIGGER organizations_keep_their_kind BEFORE UPDATE OF kind, owner_id ON organizations
WHEN NEW.kind IS NOT OLD.kind OR NEW.owner_id IS NOT OLD.owner_id
BEGIN
    SELECT RAISE(ABORT, 'an organization does not change its kind or its owner');
END;

CREATE TRIGGER organization_members_of_a_space BEFORE INSERT ON organization_members
WHEN EXISTS (SELECT 1 FROM organizations AS o
             WHERE o.id = NEW.org_id AND o.kind = 'personal' AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;
CREATE TRIGGER organization_members_stay_of_a_space BEFORE UPDATE OF org_id, user_id
    ON organization_members
WHEN EXISTS (SELECT 1 FROM organizations AS o
             WHERE o.id = NEW.org_id AND o.kind = 'personal' AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;

CREATE TRIGGER workspace_members_of_a_space BEFORE INSERT ON workspace_members
WHEN EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
             WHERE w.id = NEW.workspace_id AND o.kind = 'personal'
               AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;
CREATE TRIGGER workspace_members_stay_of_a_space BEFORE UPDATE OF workspace_id, user_id
    ON workspace_members
WHEN EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
             WHERE w.id = NEW.workspace_id AND o.kind = 'personal'
               AND o.owner_id IS NOT NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'the space of an account has its owner in it and nobody else');
END;

CREATE TRIGGER invitations_of_a_space BEFORE INSERT ON invitations
WHEN EXISTS (SELECT 1 FROM organizations AS o
             WHERE o.kind = 'personal'
               AND (o.id = NEW.org_id
                    OR o.id = (SELECT w.org_id FROM workspaces AS w
                               WHERE w.id = NEW.workspace_id)
                    OR o.id = (SELECT w.org_id FROM containers AS c
                               JOIN workspaces AS w ON w.id = c.workspace_id
                               WHERE c.id = NEW.container_id)))
BEGIN
    SELECT RAISE(ABORT, 'nobody is invited into the space of an account');
END;
CREATE TRIGGER invitations_stay_out_of_a_space
    BEFORE UPDATE OF org_id, workspace_id, container_id ON invitations
WHEN EXISTS (SELECT 1 FROM organizations AS o
             WHERE o.kind = 'personal'
               AND (o.id = NEW.org_id
                    OR o.id = (SELECT w.org_id FROM workspaces AS w
                               WHERE w.id = NEW.workspace_id)
                    OR o.id = (SELECT w.org_id FROM containers AS c
                               JOIN workspaces AS w ON w.id = c.workspace_id
                               WHERE c.id = NEW.container_id)))
BEGIN
    SELECT RAISE(ABORT, 'nobody is invited into the space of an account');
END;

CREATE TRIGGER diaries_of_a_space BEFORE INSERT ON containers
WHEN NEW.kind = 'diary'
     AND NOT EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
                     WHERE w.id = NEW.workspace_id AND o.kind = 'personal')
BEGIN
    SELECT RAISE(ABORT, 'a diary is of the space of an account: no other workspace has one');
END;
CREATE TRIGGER diaries_stay_of_a_space BEFORE UPDATE OF workspace_id, kind ON containers
WHEN NEW.kind = 'diary'
     AND NOT EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
                     WHERE w.id = NEW.workspace_id AND o.kind = 'personal')
BEGIN
    SELECT RAISE(ABORT, 'a diary is of the space of an account: no other workspace has one');
END;
