-- Courses: a fourth kind of container ('course'), whose blocks are chapters ('chapter')
-- and units ('unit').

-- Drop the triggers from 0055 that name `kind` so SQLite can drop and replace the column.
DROP TRIGGER diaries_of_a_space;
DROP TRIGGER diaries_stay_of_a_space;
DROP TRIGGER sharing_of_no_diary;
DROP TRIGGER sharing_stays_of_no_diary;

-- Widen `containers.kind` in place without rebuilding the table, as 0055 did.
ALTER TABLE containers ADD COLUMN kind_wider TEXT NOT NULL DEFAULT 'board'
    CHECK (kind_wider IN ('board', 'wiki', 'diary', 'course'));
UPDATE containers SET kind_wider = kind;
ALTER TABLE containers DROP COLUMN kind;
ALTER TABLE containers RENAME COLUMN kind_wider TO kind;

-- Recreate the four triggers from 0055 verbatim.
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

CREATE TRIGGER sharing_of_no_diary BEFORE INSERT ON sharing
WHEN EXISTS (SELECT 1 FROM containers AS c WHERE c.id = NEW.container_id AND c.kind = 'diary')
BEGIN
    SELECT RAISE(ABORT, 'a diary has no link');
END;
CREATE TRIGGER sharing_stays_of_no_diary BEFORE UPDATE OF container_id ON sharing
WHEN EXISTS (SELECT 1 FROM containers AS c WHERE c.id = NEW.container_id AND c.kind = 'diary')
BEGIN
    SELECT RAISE(ABORT, 'a diary has no link');
END;
